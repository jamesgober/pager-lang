//! Unix backend: anonymous `mmap` regions managed with `mprotect` and `munmap`.
//!
//! Regions are private anonymous mappings — memory backed by no file, visible only to this
//! process. The kernel picks the address (the first argument to `mmap` is null), so every
//! region is page-aligned by construction.

use core::ffi::{c_int, c_void};
use core::ptr::{self, NonNull};

use crate::protection::Protection;

// Page-protection bits, identical across the Unix targets this crate supports.
const PROT_NONE: c_int = 0;
const PROT_READ: c_int = 1;
const PROT_WRITE: c_int = 2;
const PROT_EXEC: c_int = 4;

// A private mapping: writes stay local to this process and are never flushed to a file.
const MAP_PRIVATE: c_int = 0x02;

// The "anonymous" flag — memory not backed by a file — has different values per kernel.
#[cfg(any(target_os = "linux", target_os = "android"))]
const MAP_ANON: c_int = 0x0020;
#[cfg(not(any(target_os = "linux", target_os = "android")))]
const MAP_ANON: c_int = 0x1000;

unsafe extern "C" {
    fn mmap(
        addr: *mut c_void,
        length: usize,
        prot: c_int,
        flags: c_int,
        fd: c_int,
        offset: i64,
    ) -> *mut c_void;
    fn mprotect(addr: *mut c_void, len: usize, prot: c_int) -> c_int;
    fn munmap(addr: *mut c_void, length: usize) -> c_int;
    fn getpagesize() -> c_int;
}

// `errno` is a thread-local set by a failing syscall. Its access path is libc-specific.
#[cfg(any(target_os = "linux", target_os = "android"))]
unsafe extern "C" {
    fn __errno_location() -> *mut c_int;
}
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "tvos",
    target_os = "watchos"
))]
unsafe extern "C" {
    fn __error() -> *mut c_int;
}

/// Reads the calling thread's `errno`, the OS error code of the most recent failed call.
fn errno() -> i32 {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        // SAFETY: `__errno_location` returns a valid, non-null pointer to this thread's
        // `errno`; reading it is a plain aligned load of an `int`.
        unsafe { *__errno_location() }
    }
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos"
    ))]
    {
        // SAFETY: `__error` returns a valid, non-null pointer to this thread's `errno`.
        unsafe { *__error() }
    }
    // On any other Unix the `errno` access path is unknown to this crate; the operation
    // still failed, it is only the numeric code that is unavailable, so report 0.
    #[cfg(not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos"
    )))]
    {
        0
    }
}

/// Translates a [`Protection`] into the `PROT_*` bitmask `mmap`/`mprotect` expect.
fn prot_bits(protection: Protection) -> c_int {
    match protection {
        Protection::None => PROT_NONE,
        Protection::Read => PROT_READ,
        Protection::ReadWrite => PROT_READ | PROT_WRITE,
        Protection::ReadExecute => PROT_READ | PROT_EXEC,
        Protection::ReadWriteExecute => PROT_READ | PROT_WRITE | PROT_EXEC,
    }
}

/// Returns true when `mmap` returned its failure sentinel, `MAP_FAILED` — `(void *) -1`.
fn is_map_failed(ret: *mut c_void) -> bool {
    ret.is_null() || ret.addr() == usize::MAX
}

/// The host page size in bytes.
pub(crate) fn page_size() -> usize {
    // SAFETY: `getpagesize` takes no arguments, touches no memory, and is always callable.
    let size = unsafe { getpagesize() };
    if size > 0 { size as usize } else { 4096 }
}

/// Maps a fresh, committed, read/write region of `length` bytes.
pub(crate) fn map(length: usize) -> Result<NonNull<u8>, i32> {
    // SAFETY: a brand-new anonymous mapping. `addr` is null so the kernel chooses the
    // address; `fd` is -1 and `offset` 0 as `MAP_ANON` requires. `mmap` is sound to call
    // with these arguments and signals failure through its return value, checked below.
    let ret = unsafe {
        mmap(
            ptr::null_mut(),
            length,
            PROT_READ | PROT_WRITE,
            MAP_PRIVATE | MAP_ANON,
            -1,
            0,
        )
    };
    if is_map_failed(ret) {
        return Err(errno());
    }
    // SAFETY: `ret` is non-null (checked) and addresses `length` valid bytes.
    Ok(unsafe { NonNull::new_unchecked(ret.cast::<u8>()) })
}

/// Reserves `total` bytes with both ends inaccessible, then makes the `data_len` bytes
/// starting `guard` bytes in read/write. Returns the base of the whole mapping.
pub(crate) fn map_guarded(total: usize, guard: usize, data_len: usize) -> Result<NonNull<u8>, i32> {
    // SAFETY: a fresh anonymous mapping (see `map`), here created with no access at all so
    // the guard pages start out unreadable, unwritable, and unexecutable.
    let base = unsafe {
        mmap(
            ptr::null_mut(),
            total,
            PROT_NONE,
            MAP_PRIVATE | MAP_ANON,
            -1,
            0,
        )
    };
    if is_map_failed(base) {
        return Err(errno());
    }
    let data = base.wrapping_byte_add(guard);
    // SAFETY: `[data, data + data_len)` is a sub-range of the mapping just created (the
    // caller guarantees `guard + data_len <= total`); raising the protection of a sub-range
    // of an owned mapping is sound.
    let rc = unsafe { mprotect(data, data_len, PROT_READ | PROT_WRITE) };
    if rc != 0 {
        let code = errno();
        // SAFETY: `base`/`total` name the mapping created just above; release it so a
        // failed setup leaks nothing.
        let _ = unsafe { munmap(base, total) };
        return Err(code);
    }
    // SAFETY: `base` is non-null (checked).
    Ok(unsafe { NonNull::new_unchecked(base.cast::<u8>()) })
}

/// Changes the protection of `len` committed bytes starting at `ptr`.
///
/// # Safety
///
/// `ptr` and `len` must name a committed sub-range of a live mapping created by [`map`] or
/// [`map_guarded`], with `len` a whole number of pages.
pub(crate) unsafe fn protect(ptr: *mut u8, len: usize, protection: Protection) -> Result<(), i32> {
    // SAFETY: the caller guarantees `ptr`/`len` describe a committed range of a live mapping.
    let rc = unsafe { mprotect(ptr.cast::<c_void>(), len, prot_bits(protection)) };
    if rc == 0 { Ok(()) } else { Err(errno()) }
}

/// Returns the whole mapping `[base, base + total)` to the operating system.
///
/// # Safety
///
/// `base` and `total` must be exactly the base and total length of a mapping returned by
/// [`map`] or [`map_guarded`] that has not already been unmapped.
pub(crate) unsafe fn unmap(base: *mut u8, total: usize) -> Result<(), i32> {
    // SAFETY: the caller guarantees `base`/`total` name a live, not-yet-freed mapping.
    let rc = unsafe { munmap(base.cast::<c_void>(), total) };
    if rc == 0 { Ok(()) } else { Err(errno()) }
}
