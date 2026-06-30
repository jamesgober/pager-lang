<h1 align="center">
    <img width="99" alt="Rust logo" src="https://raw.githubusercontent.com/jamesgober/rust-collection/72baabd71f00e14aa9184efcb16fa3deddda3a0a/assets/rust-logo.svg">
    <br><b>pager-lang</b><br>
    <sub><sup>API REFERENCE</sup></sub>
</h1>
<div align="center">
    <sup>
        <a href="../README.md" title="Project Home"><b>HOME</b></a>
        <span>&nbsp;│&nbsp;</span>
        <span>API</span>
        <span>&nbsp;│&nbsp;</span>
        <a href="../dev/ROADMAP.md" title="Roadmap"><b>ROADMAP</b></a>
    </sup>
</div>
<br>

> **Status: pre-1.0.** The surface below is being designed across the `0.x` series and will be frozen with a SemVer promise at `1.0.0`. See [SemVer](#semver) and [`dev/ROADMAP.md`](../dev/ROADMAP.md).

Virtual and executable memory management for JIT compilers and language runtimes. pager-lang hands out page-aligned regions of memory whose access permissions — read, write, execute — can be changed on demand, with optional guard pages. It is the layer a JIT uses to get memory the CPU will execute, which ordinary heap allocations are not.

<br>

## Table of Contents

- [Installation](#installation)
- [Concepts](#concepts)
- [Public API](#public-api)
  - [`page_size`](#page_size)
  - [`Region`](#region)
    - [`Region::new`](#regionnew)
    - [`Region::with_guard_pages`](#regionwith_guard_pages)
    - [`len` / `is_empty`](#len--is_empty)
    - [`protection` / `has_guard_pages`](#protection--has_guard_pages)
    - [`as_ptr` / `as_mut_ptr`](#as_ptr--as_mut_ptr)
    - [`as_slice` / `as_mut_slice`](#as_slice--as_mut_slice)
    - [`write`](#regionwrite)
    - [`protect`](#regionprotect)
  - [`Protection`](#protection)
  - [`PagerError`](#pagererror)
- [Executing generated code](#executing-generated-code)
- [Feature flags](#feature-flags)
- [SemVer](#semver)

<br>
<hr>
<br>

## Installation

```toml
[dependencies]
pager-lang = "0.2"
```

```bash
cargo add pager-lang
```

The crate is `#![no_std]`-capable: build with `default-features = false` to drop the standard library. It reaches the operating system through direct FFI either way, so behavior is identical; a target with no operating system cannot provide the calls and will not compile.

<br>
<hr>
<br>

## Concepts

A **region** is a block of page-aligned virtual memory whose access permissions can be changed independently of the rest of the address space. Three ideas run through the whole API:

- **Pages.** The operating system manages memory in fixed-size pages (see [`page_size`](#page_size)) and applies protection at page granularity. Every region is rounded up to a whole number of pages, so a region's real size is at least what you asked for.
- **Protection.** A region carries one [`Protection`](#protection) at a time — which of read, write, and execute the CPU will allow. A region is created [`ReadWrite`](#protection); [`protect`](#regionprotect) changes it.
- **Write xor execute (W^X).** The discipline of never leaving memory writable and executable at once. The intended flow: create a region (read/write), write code into it, flip it to read+execute, then run it. The safe accessors enforce this — a region that is not writable hands out no mutable slice.

```rust
use pager_lang::{Protection, Region};

let mut region = Region::new(4096)?;          // read/write
region.write(0, &[0x90, 0xC3])?;              // write while writable
region.protect(Protection::ReadExecute)?;     // flip to executable
assert!(region.as_mut_slice().is_none());     // no longer writable
# Ok::<(), pager_lang::PagerError>(())
```

<br>
<hr>
<br>

## Public API

### `page_size`

```rust
pub fn page_size() -> usize
```

The size of a memory page on the host, in bytes. Every region is a whole number of these, and protection changes apply at page granularity, so this is the unit to reason in when sizing regions.

The value is queried from the operating system once and cached, so repeated calls are a single relaxed atomic load. It is always a power of two — typically 4096, though some systems (notably Apple Silicon) use 16384.

**Returns**

- The page size in bytes: a power of two, at least 4096 on supported platforms.

**Examples**

Size a region in whole pages:

```rust
use pager_lang::{Region, page_size};

let page = page_size();
assert!(page.is_power_of_two());

// Ask for exactly four pages.
let region = Region::new(4 * page)?;
assert_eq!(region.len(), 4 * page);
# Ok::<(), pager_lang::PagerError>(())
```

Understand the rounding [`Region::new`](#regionnew) applies:

```rust
use pager_lang::{Region, page_size};

// One byte still costs a whole page.
let region = Region::new(1)?;
assert_eq!(region.len(), page_size());
# Ok::<(), pager_lang::PagerError>(())
```

<br>

### `Region`

```rust
pub struct Region { /* private fields */ }
```

An owned region of page-aligned virtual memory, freed when it is dropped. This is the central type: a JIT or runtime writes code or data into a region, changes its protection, and (for code) executes it. Dropping the region returns its pages to the operating system — there is no manual free.

`Region` is [`Send`] and [`Sync`]. It owns its mapping the way `Box<[u8]>` owns its allocation, and every method that touches the bytes takes `&self` or `&mut self`, so the borrow checker rules out data races on the safe surface. It also implements [`Debug`], printing its address, length, protection, and guard-page status without dumping the contents.

The constructors and methods are grouped below.

<br>

#### `Region::new`

```rust
pub fn new(len: usize) -> Result<Region, PagerError>
```

Maps a new read/write region of at least `len` bytes. The length is rounded up to a whole number of [`page_size`](#page_size) bytes, the region starts out [`ReadWrite`](#protection), and the bytes are zero-initialized by the operating system.

**Parameters**

- `len` — the minimum number of bytes the region must hold. Rounded up to whole pages.

**Returns**

- `Ok(Region)` — a writable region of [`len()`](#len--is_empty) ≥ `len` bytes.
- `Err(PagerError::ZeroSize)` — `len` was zero.
- `Err(PagerError::SizeOverflow)` — rounding `len` up to whole pages overflowed `usize`.
- `Err(PagerError::Map(code))` — the operating system could not map the region.

**Examples**

Allocate and fill a scratch buffer:

```rust
use pager_lang::Region;

let mut region = Region::new(256)?;
region.write(0, b"machine code goes here")?;
assert!(region.len() >= 256);
# Ok::<(), pager_lang::PagerError>(())
```

Zero is rejected up front:

```rust
use pager_lang::{PagerError, Region};

assert!(matches!(Region::new(0), Err(PagerError::ZeroSize)));
```

<br>

#### `Region::with_guard_pages`

```rust
pub fn with_guard_pages(len: usize) -> Result<Region, PagerError>
```

Maps a new read/write region of at least `len` bytes, flanked on both sides by an inaccessible guard page. The usable region behaves exactly like one from [`new`](#regionnew); the difference is the [`None`](#protection)-protected page immediately before and after it, so a read or write that runs off either end faults at once instead of silently corrupting adjacent memory.

[`len`](#len--is_empty), [`as_ptr`](#as_ptr--as_mut_ptr), and the slice accessors all describe the usable region only — the guards are never exposed.

**Parameters**

- `len` — the minimum number of usable bytes. Rounded up to whole pages; the guard pages are additional.

**Returns**

- `Ok(Region)` — a writable, guard-flanked region; [`has_guard_pages`](#protection--has_guard_pages) returns `true`.
- `Err(PagerError::ZeroSize)` — `len` was zero.
- `Err(PagerError::SizeOverflow)` — the total size including guard pages overflowed `usize`.
- `Err(PagerError::Map(code))` — the operating system could not map the region.

**Examples**

Guard a code buffer a compiler is filling:

```rust
use pager_lang::Region;

let mut region = Region::with_guard_pages(1024)?;
assert!(region.has_guard_pages());

// Fill it to the last usable byte; one past it is rejected by the bounds check,
// long before any access could reach the guard page.
let len = region.len();
region.write(len - 1, &[0xCC])?;
assert!(region.write(len, &[0xCC]).is_err());
# Ok::<(), pager_lang::PagerError>(())
```

<br>

#### `len` / `is_empty`

```rust
pub fn len(&self) -> usize
pub fn is_empty(&self) -> bool
```

`len` is the length of the usable region in bytes — a whole number of pages, at least the size requested. `is_empty` is always `false` (a region spans at least one page); it exists so the type reads naturally alongside `len`.

**Examples**

```rust
use pager_lang::{Region, page_size};

let region = Region::new(page_size() + 1)?;
// One byte over a page rounds up to two pages.
assert_eq!(region.len(), 2 * page_size());
assert!(!region.is_empty());
# Ok::<(), pager_lang::PagerError>(())
```

<br>

#### `protection` / `has_guard_pages`

```rust
pub fn protection(&self) -> Protection
pub fn has_guard_pages(&self) -> bool
```

`protection` returns the region's current [`Protection`](#protection). `has_guard_pages` reports whether the region was created with [`with_guard_pages`](#regionwith_guard_pages).

**Examples**

```rust
use pager_lang::{Protection, Region};

let plain = Region::new(64)?;
assert_eq!(plain.protection(), Protection::ReadWrite);
assert!(!plain.has_guard_pages());

let guarded = Region::with_guard_pages(64)?;
assert!(guarded.has_guard_pages());
# Ok::<(), pager_lang::PagerError>(())
```

<br>

#### `as_ptr` / `as_mut_ptr`

```rust
pub fn as_ptr(&self) -> *const u8
pub fn as_mut_ptr(&mut self) -> *mut u8
```

Raw pointers to the start of the usable region. Always valid to obtain; dereferencing or transmuting them is `unsafe` and the caller's responsibility. `as_ptr` is what you transmute to a function pointer to run code in the region (see [Executing generated code](#executing-generated-code)); `as_mut_ptr` is for `unsafe` writes when the slice accessors do not fit.

**Examples**

```rust
use pager_lang::Region;

let region = Region::new(64)?;
let base = region.as_ptr();
assert!(!base.is_null());
# Ok::<(), pager_lang::PagerError>(())
```

<br>

#### `as_slice` / `as_mut_slice`

```rust
pub fn as_slice(&self) -> Option<&[u8]>
pub fn as_mut_slice(&mut self) -> Option<&mut [u8]>
```

Borrow the region as a byte slice. `as_slice` returns `Some` exactly when the protection is readable (everything but [`None`](#protection)); `as_mut_slice` returns `Some` exactly when it is writable ([`ReadWrite`](#protection) or [`ReadWriteExecute`](#protection)). The `Option` is what keeps these safe: a region flipped to a no-access or execute-only protection cannot be read or written through a slice, so the safe API can never produce a slice that would fault.

Use these for bulk access; for a single placed write with bounds checking, [`write`](#regionwrite) is more convenient.

**Examples**

Bulk-fill a region, then read it back:

```rust
use pager_lang::Region;

let mut region = Region::new(8)?;
region.as_mut_slice().unwrap().fill(0xAB);
assert!(region.as_slice().unwrap().iter().all(|&b| b == 0xAB));
# Ok::<(), pager_lang::PagerError>(())
```

The accessors track protection:

```rust
use pager_lang::{Protection, Region};

let mut region = Region::new(32)?;
assert!(region.as_mut_slice().is_some());

region.protect(Protection::ReadExecute)?;
assert!(region.as_mut_slice().is_none());   // not writable
assert!(region.as_slice().is_some());       // still readable

region.protect(Protection::None)?;
assert!(region.as_slice().is_none());        // not even readable
# Ok::<(), pager_lang::PagerError>(())
```

<br>

#### `Region::write`

```rust
pub fn write(&mut self, offset: usize, bytes: &[u8]) -> Result<(), PagerError>
```

Copies `bytes` into the region starting at `offset`. The common way to load machine code or data: it checks that the region is writable and that the write fits, then copies. Writing an empty slice is a no-op.

**Parameters**

- `offset` — the byte offset within the region to start writing at.
- `bytes` — the bytes to copy in.

**Returns**

- `Ok(())` — the bytes were written.
- `Err(PagerError::NotWritable)` — the region's protection does not permit writing.
- `Err(PagerError::OutOfBounds { .. })` — `offset + bytes.len()` exceeds [`len()`](#len--is_empty).

**Examples**

Place bytes at an offset:

```rust
use pager_lang::Region;

let mut region = Region::new(16)?;
region.write(4, &[1, 2, 3, 4])?;
assert_eq!(&region.as_slice().unwrap()[4..8], &[1, 2, 3, 4]);
# Ok::<(), pager_lang::PagerError>(())
```

A write past the end is rejected, not truncated:

```rust
use pager_lang::{PagerError, Region};

let mut region = Region::new(16)?;
let len = region.len();
assert!(matches!(
    region.write(len, &[0]),
    Err(PagerError::OutOfBounds { .. })
));
# Ok::<(), pager_lang::PagerError>(())
```

Writing to a non-writable region is an error, not a fault:

```rust
use pager_lang::{PagerError, Protection, Region};

let mut region = Region::new(16)?;
region.protect(Protection::ReadExecute)?;
assert_eq!(region.write(0, &[0]), Err(PagerError::NotWritable));
# Ok::<(), pager_lang::PagerError>(())
```

<br>

#### `Region::protect`

```rust
pub fn protect(&mut self, protection: Protection) -> Result<(), PagerError>
```

Changes the region's protection. Applies to the usable region only; any guard pages are left untouched. Setting the protection it already has is a no-op that returns `Ok`. This is the W^X flip: write code under [`ReadWrite`](#protection), then `protect` to [`ReadExecute`](#protection) before running it.

**Parameters**

- `protection` — the new [`Protection`](#protection) to apply.

**Returns**

- `Ok(())` — the protection was changed (or already matched).
- `Err(PagerError::Protect(code))` — the operating system refused the change, most often a platform that forbids writable-and-executable pages rejecting [`ReadWriteExecute`](#protection).

**Examples**

The W^X flip:

```rust
use pager_lang::{Protection, Region};

let mut region = Region::new(64)?;
region.write(0, &[0xC3])?;                  // write under ReadWrite
region.protect(Protection::ReadExecute)?;   // flip to executable
assert_eq!(region.protection(), Protection::ReadExecute);
# Ok::<(), pager_lang::PagerError>(())
```

Lock a region down to no access, then reopen it:

```rust
use pager_lang::{Protection, Region};

let mut region = Region::new(64)?;
region.protect(Protection::None)?;          // a guard-like state
assert!(region.as_slice().is_none());

region.protect(Protection::ReadWrite)?;     // reopen for use
assert!(region.as_mut_slice().is_some());
# Ok::<(), pager_lang::PagerError>(())
```

<br>

### `Protection`

```rust
pub enum Protection {
    None,
    Read,
    ReadWrite,
    ReadExecute,
    ReadWriteExecute,
}
```

The access a page range permits: which of read, write, and execute the CPU will allow. The five cases map directly onto the platform primitives (`PROT_*` for `mprotect`, `PAGE_*` for `VirtualProtect`).

`Protection` is `Copy`, `Eq`, `Hash`, and `Debug`, and (behind the `serde` feature) `Serialize` / `Deserialize`.

**Variants**

| Variant | Read | Write | Execute | Use |
|---|:---:|:---:|:---:|---|
| `None` | ✗ | ✗ | ✗ | Guard pages; a fully locked region. |
| `Read` | ✓ | ✗ | ✗ | Constant data that must not change once written. |
| `ReadWrite` | ✓ | ✓ | ✗ | The state a new region starts in, for writing code or data. |
| `ReadExecute` | ✓ | ✗ | ✓ | Finished JIT code: runnable but no longer writable. |
| `ReadWriteExecute` | ✓ | ✓ | ✓ | Writable and executable at once. Defeats W^X; some hardened platforms refuse it. |

**Methods**

```rust
pub const fn is_readable(self) -> bool
pub const fn is_writable(self) -> bool
pub const fn is_executable(self) -> bool
```

Each reports whether the corresponding access is permitted. [`Region::as_slice`](#as_slice--as_mut_slice) succeeds when `is_readable`, and [`Region::as_mut_slice`](#as_slice--as_mut_slice) / [`Region::write`](#regionwrite) succeed when `is_writable`.

**Examples**

Inspect a protection:

```rust
use pager_lang::Protection;

assert!(Protection::ReadWrite.is_writable());
assert!(!Protection::ReadWrite.is_executable());

assert!(Protection::ReadExecute.is_executable());
assert!(!Protection::ReadExecute.is_writable());

assert!(!Protection::None.is_readable());
```

Drive a region with it:

```rust
use pager_lang::{Protection, Region};

let mut region = Region::new(64)?;
for prot in [Protection::Read, Protection::ReadExecute, Protection::ReadWrite] {
    region.protect(prot)?;
    assert_eq!(region.as_slice().is_some(), prot.is_readable());
    assert_eq!(region.as_mut_slice().is_some(), prot.is_writable());
}
# Ok::<(), pager_lang::PagerError>(())
```

<br>

### `PagerError`

```rust
#[non_exhaustive]
pub enum PagerError {
    ZeroSize,
    SizeOverflow,
    Map(i32),
    Protect(i32),
    NotWritable,
    OutOfBounds { offset: usize, len: usize, region_len: usize },
}
```

The reason a [`Region`](#region) operation failed. The variants separate a request that does not describe a valid region, the operating system refusing a call, and a write that does not fit the region as it stands. The OS-level variants carry the raw error code — `errno` on Unix, `GetLastError` on Windows — so the underlying cause is not lost.

`PagerError` implements `Display`, `std::error::Error`, `Clone`, `Eq`, and `Debug`, and (behind the `serde` feature) `Serialize` / `Deserialize`. The enum is `#[non_exhaustive]`, so a `match` on it must include a wildcard arm.

**Variants**

| Variant | Meaning | When |
|---|---|---|
| `ZeroSize` | A region of zero bytes was requested. | [`Region::new`](#regionnew) / [`with_guard_pages`](#regionwith_guard_pages) with `len == 0`. |
| `SizeOverflow` | Rounding to whole pages, or adding guard pages, overflowed `usize`. | An enormous requested size. |
| `Map(i32)` | The OS could not map the region; carries the OS error code. | Out of address space or memory. |
| `Protect(i32)` | The OS refused the protection change; carries the OS error code. | A platform forbidding writable-and-executable pages. |
| `NotWritable` | A write was attempted on a non-writable region. | [`Region::write`](#regionwrite) when the region is not [`is_writable`](#protection). |
| `OutOfBounds { offset, len, region_len }` | A write would fall outside the region. | [`Region::write`](#regionwrite) past the end. |

**Examples**

Match on the failure:

```rust
use pager_lang::{PagerError, Region};

match Region::new(usize::MAX) {
    Err(PagerError::SizeOverflow) => { /* too large to map */ }
    Err(e) => panic!("unexpected error: {e}"),
    Ok(_) => panic!("a usize::MAX region should not map"),
}
```

Read the OS error code on a real failure:

```rust
use pager_lang::{PagerError, Region};

// Whatever the cause, the code is preserved for diagnostics.
if let Err(PagerError::Map(code)) = Region::new(usize::MAX / 2) {
    eprintln!("map failed with os error {code}");
}
```

Use it as a `std::error::Error`:

```rust
use pager_lang::PagerError;
use std::error::Error;

let err: &dyn Error = &PagerError::NotWritable;
assert!(err.to_string().contains("not writable"));
```

<br>
<hr>
<br>

## Executing generated code

The safe surface gets memory into the right state; the final step — transmuting [`as_ptr`](#as_ptr--as_mut_ptr) to a function pointer and calling it — is inherently `unsafe`, because the bytes must be valid code for the target and the signature you transmute to must match the code's calling convention. That part is the caller's responsibility.

On x86-64, writing code and running it is straightforward (instruction and data caches are coherent):

```rust
# #[cfg(target_arch = "x86_64")]
# fn jit() -> Result<(), pager_lang::PagerError> {
use pager_lang::{Protection, Region};

// mov eax, 42 ; ret   (returns 42 in rax under the C calling convention)
const CODE: [u8; 6] = [0xB8, 0x2A, 0x00, 0x00, 0x00, 0xC3];

let mut region = Region::new(CODE.len())?;
region.write(0, &CODE)?;
region.protect(Protection::ReadExecute)?;

// SAFETY: the region holds a valid no-argument x86-64 function returning in rax, the
// region is read+execute, and x86-64 caches are coherent.
let f: extern "C" fn() -> u64 = unsafe { core::mem::transmute(region.as_ptr()) };
assert_eq!(f(), 42);
# Ok(())
# }
# #[cfg(target_arch = "x86_64")]
# jit().unwrap();
```

On architectures with separate instruction and data caches (for example AArch64), freshly written code also needs the instruction cache synchronized before it is executed. This crate manages the memory and its protections; that synchronization is the caller's responsibility at the point of execution. A complete runnable example lives in [`examples/jit_function.rs`](../examples/jit_function.rs).

<br>
<hr>
<br>

## Feature flags

| Feature | Default | Description |
|---|---|---|
| `std` | on | Links the standard library. Without it the crate is `#![no_std]` and needs only `core`; OS calls go through FFI either way, so behavior is identical. |
| `serde` | off | Derives `Serialize` / `Deserialize` for [`Protection`](#protection) and [`PagerError`](#pagererror). A live [`Region`](#region) is not serializable — it is an OS resource, not a value. |

```toml
# no_std build:
pager-lang = { version = "0.2", default-features = false }

# with serialization of the value types:
pager-lang = { version = "0.2", features = ["serde"] }
```

<br>

## SemVer

This crate is **pre-1.0**. The public surface above may change between minor (`0.x`) versions while it is being designed; it will be frozen and given a SemVer stability promise at `1.0.0`. Pin a `0.2` range and consult the [`CHANGELOG`](../CHANGELOG.md) when upgrading.

<br>
<hr>

<sub>Copyright &copy; 2026 <strong>James Gober</strong>.</sub>
