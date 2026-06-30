//! The owned virtual-memory region and the host page-size query.

use core::ptr::NonNull;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::error::PagerError;
use crate::protection::Protection;
use crate::sys;

/// The size of a memory page on the host, in bytes.
///
/// Every region is a whole number of these, and protection changes apply at page
/// granularity, so this is the unit to reason in when sizing regions. The value is queried
/// from the operating system once and cached, so repeated calls are a single relaxed atomic
/// load. It is always a power of two — typically 4096, though some systems use 16384.
///
/// # Examples
///
/// ```
/// let page = pager_lang::page_size();
/// assert!(page.is_power_of_two());
/// assert!(page >= 4096);
/// ```
#[must_use]
pub fn page_size() -> usize {
    static CACHE: AtomicUsize = AtomicUsize::new(0);

    let cached = CACHE.load(Ordering::Relaxed);
    if cached != 0 {
        return cached;
    }
    // Querying twice from two threads is harmless: both observe the same value and store it.
    let queried = sys::page_size();
    CACHE.store(queried, Ordering::Relaxed);
    queried
}

/// Rounds `len` up to a whole number of pages, rejecting zero and reporting overflow.
fn pages_for(len: usize) -> Result<usize, PagerError> {
    if len == 0 {
        return Err(PagerError::ZeroSize);
    }
    let page = page_size();
    // `page` is a power of two, so masking off the low bits rounds down; adding `page - 1`
    // first rounds up. The add is the only step that can overflow.
    let bumped = len.checked_add(page - 1).ok_or(PagerError::SizeOverflow)?;
    Ok(bumped & !(page - 1))
}

/// An owned region of page-aligned virtual memory, freed when it is dropped.
///
/// A region is the unit a JIT or runtime works in: a block of memory whose access
/// permissions can be changed independently of the rest of the address space. It is created
/// [`ReadWrite`](Protection::ReadWrite) so code or data can be written in, then moved to
/// another protection — usually [`ReadExecute`](Protection::ReadExecute) — with
/// [`protect`](Self::protect). Dropping the region returns its pages to the operating system.
///
/// The requested length is rounded up to a whole number of [`page_size`] bytes, so
/// [`len`](Self::len) is the real, page-aligned size and is at least what was asked for.
/// Optionally a region can be flanked by inaccessible *guard pages* with
/// [`with_guard_pages`](Self::with_guard_pages), so a run off either end faults at once
/// instead of silently corrupting a neighbour.
///
/// `Region` is [`Send`] and [`Sync`]: it owns its mapping the way `Box<[u8]>` owns its
/// allocation, and every method that touches the bytes takes `&self` or `&mut self`, so the
/// borrow checker rules out data races on the safe surface.
///
/// # Executing code
///
/// Writing machine code into a region and running it is inherently unsafe — the bytes must
/// be valid code for the target and match the calling convention you transmute to. The safe
/// surface here gets the memory into the right state; the final transmute-and-call is the
/// caller's `unsafe` responsibility. On architectures with separate instruction and data
/// caches (for example AArch64), freshly written code also needs the instruction cache
/// synchronized before it is run; on x86 and x86-64 this is automatic.
///
/// # Examples
///
/// Write a byte pattern, read it back, then flip the region to executable (the W^X pattern,
/// minus the architecture-specific call):
///
/// ```
/// use pager_lang::{Protection, Region};
///
/// let mut region = Region::new(64)?;
/// assert!(region.len() >= 64);
/// assert_eq!(region.protection(), Protection::ReadWrite);
///
/// // Fill the first bytes and read them straight back.
/// region.write(0, &[0x90, 0x90, 0xC3])?;
/// assert_eq!(&region.as_slice().unwrap()[..3], &[0x90, 0x90, 0xC3]);
///
/// // Make it read+execute: it is no longer writable.
/// region.protect(Protection::ReadExecute)?;
/// assert!(region.as_mut_slice().is_none());
/// # Ok::<(), pager_lang::PagerError>(())
/// ```
pub struct Region {
    /// Base of the whole mapping. Equal to `data` for a plain region; one page lower (the
    /// first guard page) for a guarded one. This is what `Drop` frees.
    base: NonNull<u8>,
    /// Start of the usable bytes the public methods expose.
    data: NonNull<u8>,
    /// Length of the usable region, in bytes — a whole number of pages.
    len: usize,
    /// Length of the whole mapping including any guard pages, in bytes.
    total: usize,
    /// Current protection of the usable region.
    prot: Protection,
    /// Whether guard pages flank the usable region.
    guarded: bool,
}

// SAFETY: a `Region` owns a unique virtual-memory mapping and frees it exactly once on drop.
// The raw pointers it holds are an owned resource, not aliases shared with anything else, so
// moving a `Region` to another thread simply moves ownership of that mapping — the same as
// `Box<[u8]>`. Every method that reads or writes the bytes takes `&self` or `&mut self`, so
// Rust's borrow rules prevent a shared `&Region` from being used to mutate, leaving only
// race-free shared reads.
unsafe impl Send for Region {}
// SAFETY: see the `Send` impl above. Sharing `&Region` across threads exposes only the
// read-only and pointer-returning methods; mutation requires `&mut Region`, which cannot be
// aliased, so concurrent access through shared references is data-race free.
unsafe impl Sync for Region {}

impl Region {
    /// Maps a new read/write region of at least `len` bytes.
    ///
    /// The length is rounded up to a whole number of [`page_size`] bytes; the region starts
    /// out [`ReadWrite`](Protection::ReadWrite). The bytes are zero-initialized by the
    /// operating system.
    ///
    /// # Errors
    ///
    /// - [`PagerError::ZeroSize`] if `len` is zero.
    /// - [`PagerError::SizeOverflow`] if rounding `len` up to whole pages overflows `usize`.
    /// - [`PagerError::Map`] if the operating system cannot map the region.
    ///
    /// # Examples
    ///
    /// ```
    /// use pager_lang::Region;
    ///
    /// let region = Region::new(1)?;
    /// // One byte was asked for; a whole page is what you get.
    /// assert_eq!(region.len(), pager_lang::page_size());
    /// # Ok::<(), pager_lang::PagerError>(())
    /// ```
    pub fn new(len: usize) -> Result<Self, PagerError> {
        let data_len = pages_for(len)?;
        let base = sys::map(data_len).map_err(PagerError::Map)?;
        Ok(Region {
            base,
            data: base,
            len: data_len,
            total: data_len,
            prot: Protection::ReadWrite,
            guarded: false,
        })
    }

    /// Maps a new read/write region of at least `len` bytes, flanked on both sides by an
    /// inaccessible guard page.
    ///
    /// The usable region behaves exactly like one from [`new`](Self::new): rounded up to
    /// whole pages and starting [`ReadWrite`](Protection::ReadWrite). The difference is the
    /// guard page immediately before and after it — both [`None`](Protection::None) — so a
    /// read or write that runs off either end faults immediately rather than corrupting
    /// whatever happened to be mapped next. [`has_guard_pages`](Self::has_guard_pages)
    /// reports `true`. [`len`](Self::len), [`as_ptr`](Self::as_ptr), and the slice accessors
    /// all describe the usable region only; the guards are never exposed.
    ///
    /// # Errors
    ///
    /// - [`PagerError::ZeroSize`] if `len` is zero.
    /// - [`PagerError::SizeOverflow`] if the total size including guard pages overflows `usize`.
    /// - [`PagerError::Map`] if the operating system cannot map the region.
    ///
    /// # Examples
    ///
    /// ```
    /// use pager_lang::Region;
    ///
    /// let region = Region::with_guard_pages(128)?;
    /// assert!(region.has_guard_pages());
    /// assert!(region.len() >= 128);
    /// # Ok::<(), pager_lang::PagerError>(())
    /// ```
    pub fn with_guard_pages(len: usize) -> Result<Self, PagerError> {
        let page = page_size();
        let data_len = pages_for(len)?;
        let two_guards = page.checked_mul(2).ok_or(PagerError::SizeOverflow)?;
        let total = data_len
            .checked_add(two_guards)
            .ok_or(PagerError::SizeOverflow)?;
        let base = sys::map_guarded(total, page, data_len).map_err(PagerError::Map)?;
        // SAFETY: the usable region starts `page` bytes into a mapping of `total` bytes, and
        // `page < total`, so the offset pointer stays inside the same mapping and is non-null.
        let data = unsafe { NonNull::new_unchecked(base.as_ptr().add(page)) };
        Ok(Region {
            base,
            data,
            len: data_len,
            total,
            prot: Protection::ReadWrite,
            guarded: true,
        })
    }

    /// The length of the usable region in bytes — a whole number of pages, at least the size
    /// requested.
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the region is empty. Always `false`: a region spans at least one page. Present
    /// so the type reads naturally alongside [`len`](Self::len).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The region's current protection.
    #[must_use]
    pub fn protection(&self) -> Protection {
        self.prot
    }

    /// Whether the region is flanked by guard pages (created with
    /// [`with_guard_pages`](Self::with_guard_pages)).
    #[must_use]
    pub fn has_guard_pages(&self) -> bool {
        self.guarded
    }

    /// A raw const pointer to the start of the usable region.
    ///
    /// Always valid to obtain; dereferencing or transmuting it (for example to a function
    /// pointer to run code in the region) is `unsafe` and the caller's responsibility.
    #[must_use]
    pub fn as_ptr(&self) -> *const u8 {
        self.data.as_ptr()
    }

    /// A raw mutable pointer to the start of the usable region.
    ///
    /// Always valid to obtain; dereferencing it is `unsafe` and only sound while the region's
    /// protection permits the access.
    #[must_use]
    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.data.as_ptr()
    }

    /// Borrows the region as a byte slice, or `None` if it is not currently readable.
    ///
    /// Returns `Some` exactly when [`protection`](Self::protection) is readable (everything
    /// but [`Protection::None`]). The `None` case is what keeps this safe: a region flipped
    /// to a no-access protection cannot be read through a slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use pager_lang::{Protection, Region};
    ///
    /// let mut region = Region::new(32)?;
    /// assert!(region.as_slice().is_some());
    ///
    /// region.protect(Protection::None)?;
    /// assert!(region.as_slice().is_none());
    /// # Ok::<(), pager_lang::PagerError>(())
    /// ```
    #[must_use]
    pub fn as_slice(&self) -> Option<&[u8]> {
        if !self.prot.is_readable() {
            return None;
        }
        // SAFETY: `data` addresses `len` bytes of a live, readable mapping; `len` is a whole
        // number of pages and so well under `isize::MAX`. The shared borrow of `self` bounds
        // the slice's lifetime, and a readable protection cannot change without `&mut self`.
        Some(unsafe { core::slice::from_raw_parts(self.data.as_ptr(), self.len) })
    }

    /// Borrows the region as a mutable byte slice, or `None` if it is not currently writable.
    ///
    /// Returns `Some` exactly when [`protection`](Self::protection) is writable
    /// ([`ReadWrite`](Protection::ReadWrite) or
    /// [`ReadWriteExecute`](Protection::ReadWriteExecute)). Use this to fill a region in bulk;
    /// for a single placed write, [`write`](Self::write) does the bounds checking for you.
    ///
    /// # Examples
    ///
    /// ```
    /// use pager_lang::{Protection, Region};
    ///
    /// let mut region = Region::new(8)?;
    /// region.as_mut_slice().unwrap().fill(0xAB);
    /// assert!(region.as_slice().unwrap().iter().all(|&b| b == 0xAB));
    ///
    /// region.protect(Protection::ReadExecute)?;
    /// assert!(region.as_mut_slice().is_none());
    /// # Ok::<(), pager_lang::PagerError>(())
    /// ```
    #[must_use]
    pub fn as_mut_slice(&mut self) -> Option<&mut [u8]> {
        if !self.prot.is_writable() {
            return None;
        }
        // SAFETY: `data` addresses `len` writable bytes of a live mapping; `len` is well
        // under `isize::MAX`. The exclusive borrow of `self` guarantees no other reference to
        // these bytes exists for the slice's lifetime.
        Some(unsafe { core::slice::from_raw_parts_mut(self.data.as_ptr(), self.len) })
    }

    /// Copies `bytes` into the region starting at `offset`.
    ///
    /// The common way to load machine code or data: it checks that the region is writable and
    /// that the write fits, then copies. Writing an empty slice is a no-op.
    ///
    /// # Errors
    ///
    /// - [`PagerError::NotWritable`] if the region's protection does not permit writing.
    /// - [`PagerError::OutOfBounds`] if `offset + bytes.len()` exceeds [`len`](Self::len).
    ///
    /// # Examples
    ///
    /// ```
    /// use pager_lang::{PagerError, Region};
    ///
    /// let mut region = Region::new(16)?;
    /// region.write(4, &[1, 2, 3, 4])?;
    /// assert_eq!(&region.as_slice().unwrap()[4..8], &[1, 2, 3, 4]);
    ///
    /// // A write past the end is rejected, not truncated.
    /// let len = region.len();
    /// assert!(matches!(
    ///     region.write(len, &[0]),
    ///     Err(PagerError::OutOfBounds { .. })
    /// ));
    /// # Ok::<(), pager_lang::PagerError>(())
    /// ```
    pub fn write(&mut self, offset: usize, bytes: &[u8]) -> Result<(), PagerError> {
        if !self.prot.is_writable() {
            return Err(PagerError::NotWritable);
        }
        let out_of_bounds = || PagerError::OutOfBounds {
            offset,
            len: bytes.len(),
            region_len: self.len,
        };
        let end = offset.checked_add(bytes.len()).ok_or_else(out_of_bounds)?;
        if end > self.len {
            return Err(out_of_bounds());
        }
        if bytes.is_empty() {
            return Ok(());
        }
        // SAFETY: the destination range `[offset, offset + bytes.len())` lies within the
        // writable region (bounds checked just above). The source is the caller's slice and
        // the destination is our mapping, so the two never overlap; both are valid for the
        // length of the copy.
        unsafe {
            core::ptr::copy_nonoverlapping(
                bytes.as_ptr(),
                self.data.as_ptr().add(offset),
                bytes.len(),
            );
        }
        Ok(())
    }

    /// Changes the region's protection.
    ///
    /// Applies to the usable region only; any guard pages are left untouched. Setting the
    /// protection it already has is a no-op that returns `Ok`. This is the W^X flip: write
    /// code under [`ReadWrite`](Protection::ReadWrite), then `protect` to
    /// [`ReadExecute`](Protection::ReadExecute) before running it.
    ///
    /// # Errors
    ///
    /// [`PagerError::Protect`] if the operating system refuses the change — most often a
    /// platform that forbids writable-and-executable pages rejecting
    /// [`Protection::ReadWriteExecute`].
    ///
    /// # Examples
    ///
    /// ```
    /// use pager_lang::{Protection, Region};
    ///
    /// let mut region = Region::new(64)?;
    /// region.protect(Protection::ReadExecute)?;
    /// assert_eq!(region.protection(), Protection::ReadExecute);
    /// # Ok::<(), pager_lang::PagerError>(())
    /// ```
    pub fn protect(&mut self, protection: Protection) -> Result<(), PagerError> {
        if protection == self.prot {
            return Ok(());
        }
        // SAFETY: `data`/`len` name the committed usable region of a live mapping this
        // `Region` owns, with `len` a whole number of pages — exactly what `sys::protect`
        // requires.
        unsafe { sys::protect(self.data.as_ptr(), self.len, protection) }
            .map_err(PagerError::Protect)?;
        self.prot = protection;
        Ok(())
    }
}

impl core::fmt::Debug for Region {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Region")
            .field("addr", &self.data.as_ptr())
            .field("len", &self.len)
            .field("protection", &self.prot)
            .field("guard_pages", &self.guarded)
            .finish()
    }
}

impl Drop for Region {
    fn drop(&mut self) {
        // SAFETY: `base`/`total` are exactly the base and length of the mapping this `Region`
        // created and uniquely owns; `Drop` runs once, so the mapping has not been freed.
        // The result is deliberately discarded: `Drop` cannot return an error and there is no
        // meaningful recovery from a failed unmap.
        let _ = unsafe { sys::unmap(self.base.as_ptr(), self.total) };
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests assert on specific outcomes; a wrong outcome should fail the test loudly"
)]
mod tests {
    use super::{Region, page_size};
    use crate::{PagerError, Protection};

    #[test]
    fn test_page_size_is_a_sane_power_of_two() {
        let page = page_size();
        assert!(page.is_power_of_two());
        assert!(page >= 4096);
        // Cached: a second call returns the same value.
        assert_eq!(page, page_size());
    }

    #[test]
    fn test_new_rounds_up_to_a_whole_page() {
        let region = Region::new(1).unwrap();
        assert_eq!(region.len(), page_size());
        assert!(!region.is_empty());
        assert_eq!(region.protection(), Protection::ReadWrite);
        assert!(!region.has_guard_pages());
    }

    #[test]
    fn test_new_zero_is_rejected() {
        assert!(matches!(Region::new(0), Err(PagerError::ZeroSize)));
    }

    #[test]
    fn test_new_overflow_is_rejected() {
        assert!(matches!(
            Region::new(usize::MAX),
            Err(PagerError::SizeOverflow)
        ));
    }

    #[test]
    fn test_write_then_read_round_trips() {
        let mut region = Region::new(64).unwrap();
        region.write(8, &[0xDE, 0xAD, 0xBE, 0xEF]).unwrap();
        assert_eq!(
            &region.as_slice().unwrap()[8..12],
            &[0xDE, 0xAD, 0xBE, 0xEF]
        );
    }

    #[test]
    fn test_write_out_of_bounds_is_rejected() {
        let mut region = Region::new(16).unwrap();
        let len = region.len();
        assert!(matches!(
            region.write(len, &[1]),
            Err(PagerError::OutOfBounds { .. })
        ));
        // An offset that overflows when the length is added is also rejected, not wrapped.
        assert!(matches!(
            region.write(usize::MAX, &[1, 2]),
            Err(PagerError::OutOfBounds { .. })
        ));
    }

    #[test]
    fn test_write_empty_slice_is_ok_even_at_the_end() {
        let mut region = Region::new(16).unwrap();
        let len = region.len();
        assert_eq!(region.write(len, &[]), Ok(()));
    }

    #[test]
    fn test_protect_changes_access_and_gates_the_slices() {
        let mut region = Region::new(32).unwrap();
        assert!(region.as_mut_slice().is_some());

        region.protect(Protection::ReadExecute).unwrap();
        assert_eq!(region.protection(), Protection::ReadExecute);
        assert!(region.as_mut_slice().is_none());
        assert!(region.as_slice().is_some());
        assert_eq!(region.write(0, &[1]), Err(PagerError::NotWritable));

        region.protect(Protection::None).unwrap();
        assert!(region.as_slice().is_none());
    }

    #[test]
    fn test_protect_to_same_protection_is_a_noop() {
        let mut region = Region::new(8).unwrap();
        assert_eq!(region.protect(Protection::ReadWrite), Ok(()));
        assert_eq!(region.protection(), Protection::ReadWrite);
    }

    #[test]
    fn test_guarded_region_is_usable_like_any_other() {
        let mut region = Region::with_guard_pages(100).unwrap();
        assert!(region.has_guard_pages());
        assert!(region.len() >= 100);
        region.write(0, &[7; 16]).unwrap();
        assert_eq!(&region.as_slice().unwrap()[..16], &[7; 16]);
    }

    #[test]
    fn test_many_regions_coexist() {
        let regions: Vec<Region> = (1..=32).map(|n| Region::new(n * 64).unwrap()).collect();
        for (i, region) in regions.iter().enumerate() {
            assert!(region.len() >= (i + 1) * 64);
        }
        // All freed here without crashing.
    }
}
