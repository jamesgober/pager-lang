//! # pager_lang
//!
//! Virtual and executable memory management for JIT compilers and language runtimes.
//!
//! A just-in-time compiler emits machine code at run time and then jumps into it. To do that
//! it needs memory the operating system will let the CPU *execute*, which ordinary heap
//! allocations are not. pager-lang is the small, focused layer that hands out that memory:
//! page-aligned regions whose access permissions — read, write, execute — can be changed on
//! demand, with optional guard pages, and nothing more.
//!
//! The whole surface is three types and one function:
//!
//! - [`Region`] — an owned block of page-aligned virtual memory, freed when dropped.
//! - [`Protection`] — the access a region permits: read, write, execute, in the useful
//!   combinations.
//! - [`PagerError`] — the reason an operation failed.
//! - [`page_size`] — the host page size, the unit regions are measured in.
//!
//! ## Write xor execute
//!
//! The intended flow is *write xor execute* (W^X), the discipline that keeps memory from
//! ever being writable and executable at the same time. A region is born
//! [`ReadWrite`](Protection::ReadWrite); you write code into it; you flip it to
//! [`ReadExecute`](Protection::ReadExecute); then you run it. At no point is the same page
//! both writable and runnable, which closes off a large class of exploits.
//!
//! ```
//! use pager_lang::{Protection, Region};
//!
//! // Reserve a writable page.
//! let mut code = Region::new(4096)?;
//!
//! // Write some bytes (here a harmless pattern — real code is target-specific).
//! code.write(0, &[0x90, 0x90, 0xC3])?; // x86: nop; nop; ret
//!
//! // Flip to read+execute. The region is no longer writable.
//! code.protect(Protection::ReadExecute)?;
//! assert!(code.as_mut_slice().is_none());
//!
//! // `code.as_ptr()` now points at executable memory. Transmuting it to a function
//! // pointer and calling it is `unsafe` and the caller's responsibility — see
//! // `examples/jit_function.rs` for a complete, runnable example on x86-64.
//! # Ok::<(), pager_lang::PagerError>(())
//! ```
//!
//! ## Guard pages
//!
//! [`Region::with_guard_pages`] flanks the usable region with inaccessible pages, so a write
//! that runs off either end faults immediately instead of silently corrupting adjacent
//! memory — cheap insurance for a code buffer a compiler is filling.
//!
//! ## Platforms
//!
//! Regions are backed by `mmap`/`mprotect`/`munmap` on Unix and
//! `VirtualAlloc`/`VirtualProtect`/`VirtualFree` on Windows, reached through direct FFI with
//! no third-party dependencies. Linux, macOS, and Windows (x86-64 and ARM64) are the
//! supported targets; a target with no operating system cannot provide these calls and will
//! not compile.
//!
//! Getting memory into an executable state is portable; *running* freshly written code is
//! not entirely. On architectures with separate instruction and data caches — AArch64, for
//! instance — code written into a region must have the instruction cache synchronized before
//! it is executed. On x86 and x86-64 this is automatic. This crate manages the memory and
//! its protections; instruction-cache synchronization for the architectures that need it is
//! the caller's responsibility at the point of execution.
//!
//! ## Features
//!
//! - `std` (default) — links the standard library. Without it the crate is `#![no_std]` and
//!   needs only `core`; it reaches the operating system through FFI either way, so behavior
//!   is identical.
//! - `serde` — derives `Serialize` / `Deserialize` for [`Protection`] and [`PagerError`], so
//!   a region's configuration and failure reasons can cross a serialization boundary.
//!
//! ## Stability
//!
//! Pre-1.0: the public surface is still being designed and may change between minor versions
//! until it is frozen at `1.0.0`. The current scope and plan are recorded in `docs/API.md`
//! and `dev/ROADMAP.md`.

#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![deny(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::undocumented_unsafe_blocks)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::unimplemented,
    clippy::unreachable,
    clippy::dbg_macro,
    clippy::print_stdout,
    clippy::print_stderr
)]

mod error;
mod protection;
mod region;
mod sys;

pub use error::PagerError;
pub use protection::Protection;
pub use region::{Region, page_size};
