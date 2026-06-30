//! The thin platform layer over the operating system's virtual-memory calls.
//!
//! Everything above this module — [`Region`](crate::Region), [`Protection`](crate::Protection),
//! and the rest — is portable and platform-agnostic. The OS-specific work is confined here:
//! one submodule per family, each exposing the same small set of operations so the layers
//! above never name a syscall or a platform constant.
//!
//! The contract every backend upholds:
//!
//! - [`page_size`] returns the host page size in bytes (always a power of two).
//! - [`map`] reserves and commits a read/write region of a whole number of pages.
//! - [`map_guarded`] reserves a span, makes the middle read/write, and leaves the flanking
//!   guard pages inaccessible.
//! - [`protect`] changes the access permissions of a committed range.
//! - [`unmap`] returns a whole mapping to the operating system.
//!
//! Each fallible call reports the raw OS error code on failure (`errno` on Unix,
//! `GetLastError` on Windows), which the public [`PagerError`](crate::PagerError) carries
//! through to the caller.

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub(crate) use unix::{map, map_guarded, page_size, protect, unmap};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::{map, map_guarded, page_size, protect, unmap};

#[cfg(not(any(unix, windows)))]
compile_error!(
    "pager-lang maps virtual memory through the operating system (mmap / VirtualAlloc) and \
     supports Unix and Windows targets only; it cannot be built for this target."
);
