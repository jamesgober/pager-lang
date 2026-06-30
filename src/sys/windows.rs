//! Windows backend: regions managed with `VirtualAlloc`, `VirtualProtect`, `VirtualFree`.
//!
//! A plain region is reserved and committed in one `VirtualAlloc` call. A guarded region is
//! reserved as one span and only the usable middle is committed; the flanking pages are left
//! reserved-but-uncommitted, which makes any access to them fault — the Windows equivalent of
//! a `PROT_NONE` guard page, at no commit-charge cost.

use core::ffi::c_void;
use core::mem::MaybeUninit;
use core::ptr::{self, NonNull};

use crate::protection::Protection;

// Allocation-type flags for `VirtualAlloc` / `VirtualFree`.
const MEM_COMMIT: u32 = 0x0000_1000;
const MEM_RESERVE: u32 = 0x0000_2000;
const MEM_RELEASE: u32 = 0x0000_8000;

// Page-protection constants for `VirtualAlloc` / `VirtualProtect`.
const PAGE_NOACCESS: u32 = 0x01;
const PAGE_READONLY: u32 = 0x02;
const PAGE_READWRITE: u32 = 0x04;
const PAGE_EXECUTE_READ: u32 = 0x20;
const PAGE_EXECUTE_READWRITE: u32 = 0x40;

/// Mirror of the Win32 `SYSTEM_INFO` structure, used only to read `dwPageSize`.
#[repr(C)]
#[allow(
    dead_code,
    reason = "every field must be present for the struct's size and field offsets to match \
              the C layout, even though only dw_page_size is read"
)]
struct SystemInfo {
    w_processor_architecture: u16,
    w_reserved: u16,
    dw_page_size: u32,
    lp_minimum_application_address: *mut c_void,
    lp_maximum_application_address: *mut c_void,
    dw_active_processor_mask: usize,
    dw_number_of_processors: u32,
    dw_processor_type: u32,
    dw_allocation_granularity: u32,
    w_processor_level: u16,
    w_processor_revision: u16,
}

#[allow(
    non_snake_case,
    reason = "these are foreign Win32 symbols and must keep their exported names"
)]
unsafe extern "system" {
    fn VirtualAlloc(
        address: *mut c_void,
        size: usize,
        allocation_type: u32,
        protect: u32,
    ) -> *mut c_void;
    fn VirtualProtect(
        address: *mut c_void,
        size: usize,
        new_protect: u32,
        old_protect: *mut u32,
    ) -> i32;
    fn VirtualFree(address: *mut c_void, size: usize, free_type: u32) -> i32;
    fn GetSystemInfo(system_info: *mut SystemInfo);
    fn GetLastError() -> u32;
}

/// Reads the calling thread's last-error code, the OS code of the most recent failed call.
fn last_error() -> i32 {
    // SAFETY: `GetLastError` takes no arguments and only reads a thread-local code.
    unsafe { GetLastError() as i32 }
}

/// Translates a [`Protection`] into the `PAGE_*` constant the Win32 calls expect.
fn page_protect(protection: Protection) -> u32 {
    match protection {
        Protection::None => PAGE_NOACCESS,
        Protection::Read => PAGE_READONLY,
        Protection::ReadWrite => PAGE_READWRITE,
        Protection::ReadExecute => PAGE_EXECUTE_READ,
        Protection::ReadWriteExecute => PAGE_EXECUTE_READWRITE,
    }
}

/// The host page size in bytes.
pub(crate) fn page_size() -> usize {
    let mut info = MaybeUninit::<SystemInfo>::uninit();
    // SAFETY: `GetSystemInfo` writes a fully-initialized `SYSTEM_INFO` through the pointer.
    unsafe { GetSystemInfo(info.as_mut_ptr()) };
    // SAFETY: the call above initialized every field of the struct.
    let size = unsafe { info.assume_init_ref().dw_page_size } as usize;
    if size != 0 { size } else { 4096 }
}

/// Reserves and commits a fresh read/write region of `length` bytes.
pub(crate) fn map(length: usize) -> Result<NonNull<u8>, i32> {
    // SAFETY: a brand-new region; a null address lets the OS choose where to place it.
    // `VirtualAlloc` is sound to call and reports failure by returning null, checked below.
    let ret = unsafe {
        VirtualAlloc(
            ptr::null_mut(),
            length,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        )
    };
    NonNull::new(ret.cast::<u8>()).ok_or_else(last_error)
}

/// Reserves `total` bytes, commits the `data_len` bytes starting `guard` bytes in as
/// read/write, and leaves the flanking pages reserved (inaccessible). Returns the base.
pub(crate) fn map_guarded(total: usize, guard: usize, data_len: usize) -> Result<NonNull<u8>, i32> {
    // SAFETY: a fresh reservation with no committed pages; reserved memory is inaccessible,
    // which is exactly the guard behavior we want for the flanking pages.
    let base = unsafe { VirtualAlloc(ptr::null_mut(), total, MEM_RESERVE, PAGE_NOACCESS) };
    if base.is_null() {
        return Err(last_error());
    }
    let data = base.wrapping_byte_add(guard);
    // SAFETY: `[data, data + data_len)` is a sub-range of the reservation just made (the
    // caller guarantees `guard + data_len <= total`); committing a sub-range of a
    // reservation is the documented `VirtualAlloc` usage.
    let committed = unsafe { VirtualAlloc(data, data_len, MEM_COMMIT, PAGE_READWRITE) };
    if committed.is_null() {
        let code = last_error();
        // SAFETY: `base` is the reservation created just above; release it so a failed setup
        // leaks no address space. With `MEM_RELEASE` the size argument must be 0.
        let _ = unsafe { VirtualFree(base, 0, MEM_RELEASE) };
        return Err(code);
    }
    // SAFETY: `base` is non-null (checked).
    Ok(unsafe { NonNull::new_unchecked(base.cast::<u8>()) })
}

/// Changes the protection of `len` committed bytes starting at `ptr`.
///
/// # Safety
///
/// `ptr` and `len` must name a committed sub-range of a live region created by [`map`] or
/// [`map_guarded`], with `len` a whole number of pages.
pub(crate) unsafe fn protect(ptr: *mut u8, len: usize, protection: Protection) -> Result<(), i32> {
    let mut previous = 0u32;
    // SAFETY: the caller guarantees `ptr`/`len` describe a committed range of a live region;
    // `previous` is a valid out-pointer for the required old-protection value.
    let rc = unsafe {
        VirtualProtect(
            ptr.cast::<c_void>(),
            len,
            page_protect(protection),
            &mut previous,
        )
    };
    if rc != 0 { Ok(()) } else { Err(last_error()) }
}

/// Returns the whole region beginning at `base` to the operating system.
///
/// # Safety
///
/// `base` must be exactly the base address returned by [`map`] or [`map_guarded`] that has
/// not already been freed. The `_total` length is unused (`MEM_RELEASE` frees the whole
/// reservation) but kept for signature parity with the Unix backend.
pub(crate) unsafe fn unmap(base: *mut u8, _total: usize) -> Result<(), i32> {
    // SAFETY: the caller guarantees `base` is a live reservation; `MEM_RELEASE` requires a
    // size of 0 and frees the entire reservation in one call.
    let rc = unsafe { VirtualFree(base.cast::<c_void>(), 0, MEM_RELEASE) };
    if rc != 0 { Ok(()) } else { Err(last_error()) }
}
