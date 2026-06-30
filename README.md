<h1 align="center">
    <img width="99" alt="Rust logo" src="https://raw.githubusercontent.com/jamesgober/rust-collection/72baabd71f00e14aa9184efcb16fa3deddda3a0a/assets/rust-logo.svg">
    <br>
    <b>pager-lang</b>
    <br>
    <sub><sup>EXECUTABLE MEMORY</sup></sub>
</h1>

<div align="center">
    <a href="https://crates.io/crates/pager-lang"><img alt="Crates.io" src="https://img.shields.io/crates/v/pager-lang"></a>
    <a href="https://crates.io/crates/pager-lang"><img alt="Downloads" src="https://img.shields.io/crates/d/pager-lang?color=%230099ff"></a>
    <a href="https://docs.rs/pager-lang"><img alt="docs.rs" src="https://img.shields.io/docsrs/pager-lang"></a>
    <a href="https://github.com/jamesgober/pager-lang/actions"><img alt="CI" src="https://github.com/jamesgober/pager-lang/actions/workflows/ci.yml/badge.svg"></a>
    <a href="https://github.com/rust-lang/rfcs/blob/master/text/2495-min-rust-version.md"><img alt="MSRV" src="https://img.shields.io/badge/MSRV-1.85%2B-blue"></a>
</div>

<br>

<div align="left">
    <p>
        <strong>pager-lang</strong> is the CODE-tier crate of the <a href="https://github.com/jamesgober">-lang</a> language-construction family: virtual and executable memory management for JIT compilers and language runtimes. A just-in-time compiler emits machine code at run time and then jumps into it &mdash; which needs memory the operating system will let the CPU <em>execute</em>, something ordinary heap allocations are not. pager-lang is the small, focused layer that hands out that memory.
    </p>
    <p>
        It gives you page-aligned <b>regions</b> whose access permissions &mdash; read, write, execute &mdash; can be changed on demand, with optional <b>guard pages</b> and nothing more. The surface is three types and one function. Regions are backed by <code>mmap</code> on Unix and <code>VirtualAlloc</code> on Windows through direct FFI, with <b>zero third-party dependencies</b> on the core path.
    </p>
    <br>
    <hr>
    <p>
        <strong>MSRV is 1.85+</strong> (Rust 2024 edition).
    </p>
    <blockquote>
        <strong>Status: pre-1.0, in active development.</strong> The Core memory-management surface has landed (<code>v0.2.0</code>) and is documented in <a href="./docs/API.md"><code>docs/API.md</code></a>. The public API is being designed across the 0.x series and frozen at <code>1.0.0</code>. See <a href="./CHANGELOG.md"><code>CHANGELOG.md</code></a>.
    </blockquote>
</div>

<hr>
<br>

## Overview

A region is a block of page-aligned virtual memory whose access permissions can be changed independently of the rest of the address space. The intended discipline is **write xor execute** (W^X): create a region writable, fill it with code, flip it to executable, and run it &mdash; so the same page is never writable and runnable at the same time, closing off a large class of exploits.

The whole API is four things:

- [`Region`](./docs/API.md#region) &mdash; an owned block of page-aligned virtual memory, freed when dropped.
- [`Protection`](./docs/API.md#protection) &mdash; the access a region permits, in the useful combinations of read, write, and execute.
- [`PagerError`](./docs/API.md#pagererror) &mdash; the reason an operation failed, carrying the OS error code where one applies.
- [`page_size`](./docs/API.md#page_size) &mdash; the host page size, the unit regions are measured in.

<br>
<hr>
<br>

## Installation

```toml
[dependencies]
pager-lang = "0.2"
```

Or from the terminal:

```bash
cargo add pager-lang
```

<br>

## Quick Start

```rust
use pager_lang::{Protection, Region};

// 1. Reserve a writable region (rounded up to whole pages).
let mut code = Region::new(4096).expect("map a region");

// 2. Write machine code into it (here a harmless x86 pattern: nop; nop; ret).
code.write(0, &[0x90, 0x90, 0xC3]).expect("write while writable");

// 3. The W^X flip: make it read+execute. It is no longer writable.
code.protect(Protection::ReadExecute).expect("flip to executable");
assert!(code.as_mut_slice().is_none());

// 4. `code.as_ptr()` now points at executable memory. Transmuting it to a function
//    pointer and calling it is `unsafe` and the caller's responsibility — see
//    `examples/jit_function.rs` for a complete, runnable JIT on x86-64.
```

<br>
<hr>
<br>

## How it works

| Step | Call | What happens |
|---|---|---|
| Reserve | [`Region::new(len)`](./docs/API.md#regionnew) | `mmap` / `VirtualAlloc` a read/write region of `len` bytes, rounded up to whole pages. |
| Guard (optional) | [`Region::with_guard_pages(len)`](./docs/API.md#regionwith_guard_pages) | Flank the region with inaccessible pages so an overrun faults instead of corrupting memory. |
| Write | [`region.write(offset, &bytes)`](./docs/API.md#regionwrite) | Bounds-checked copy into the region while it is writable. |
| Flip | [`region.protect(Protection::ReadExecute)`](./docs/API.md#regionprotect) | `mprotect` / `VirtualProtect` to read+execute; the region stops being writable. |
| Run | `region.as_ptr()` → transmute → call | Caller's `unsafe` step: run the generated code. |
| Free | *drop* | The region returns its pages to the OS automatically. |

The safe accessors enforce the discipline: [`as_mut_slice`](./docs/API.md#as_slice--as_mut_slice) returns `None` unless the region is writable, and [`as_slice`](./docs/API.md#as_slice--as_mut_slice) returns `None` unless it is readable, so the safe API can never hand out a slice that would fault.

<br>
<hr>
<br>

## API Overview

For a complete reference with examples, see [`docs/API.md`](./docs/API.md).

- [`Region`](./docs/API.md#region) &mdash; create with [`new`](./docs/API.md#regionnew) or [`with_guard_pages`](./docs/API.md#regionwith_guard_pages); [`write`](./docs/API.md#regionwrite), [`protect`](./docs/API.md#regionprotect), borrow with [`as_slice`](./docs/API.md#as_slice--as_mut_slice) / [`as_mut_slice`](./docs/API.md#as_slice--as_mut_slice), or take a raw [`as_ptr`](./docs/API.md#as_ptr--as_mut_ptr).
- [`Protection`](./docs/API.md#protection) &mdash; `None`, `Read`, `ReadWrite`, `ReadExecute`, `ReadWriteExecute`, with `is_readable` / `is_writable` / `is_executable`.
- [`PagerError`](./docs/API.md#pagererror) &mdash; `ZeroSize`, `SizeOverflow`, `Map`, `Protect`, `NotWritable`, `OutOfBounds`.
- [`page_size`](./docs/API.md#page_size) &mdash; the cached host page size.

### Error handling

Every fallible operation returns `Result<_, PagerError>`; the crate never panics on a recoverable condition. The OS-level variants carry the raw error code (`errno` on Unix, `GetLastError` on Windows) so the underlying cause is not lost.

```rust
use pager_lang::{PagerError, Region};

match Region::new(usize::MAX) {
    Err(PagerError::SizeOverflow) => { /* too large to round to whole pages */ }
    Err(e) => eprintln!("map failed: {e}"),
    Ok(region) => { /* use it */ }
}
```

<br>
<hr>
<br>

## Performance

Performance is a hard constraint, not an afterthought (see [`REPS.md`](./REPS.md)). The hot operations are thin wrappers over a single syscall each, with no allocation of their own; [`page_size`](./docs/API.md#page_size) is queried once and cached, and [`write`](./docs/API.md#regionwrite) is a bounds-checked `memcpy`.

Latest local Criterion means (`cargo bench --bench bench --all-features`, x86-64 Linux / WSL2, Rust stable, release build):

| Operation | Size | Time |
|---|---:|---:|
| Map + free, plain | any | ~1.0 µs |
| Map + free, guarded | any | ~2.2 µs |
| Protect (read/write → read/execute → read/write) | any | ~0.32 µs |
| Write (memcpy into the region) | 64 B | ~1.2 ns |
| Write (memcpy into the region) | 4 KiB | ~20 ns |
| Write (memcpy into the region) | 64 KiB | ~0.52 µs |

Map and protect costs are dominated by the kernel transition and are essentially independent of region size; write cost scales with the bytes copied. Numbers vary by CPU and environment &mdash; run the benches on your hardware for trends.

<br>
<hr>
<br>

## Configuration

### Feature Flags

| Feature | Default | Description |
|---|---|---|
| `std` | on | Links the standard library. Without it the crate is `#![no_std]` and needs only `core`; OS calls go through FFI either way, so behavior is identical. |
| `serde` | off | Derives `Serialize` / `Deserialize` for `Protection` and `PagerError`. A live `Region` is an OS resource, not a value, and is not serializable. |

```toml
# no_std build:
pager-lang = { version = "0.2", default-features = false }

# with serialization of the value types:
pager-lang = { version = "0.2", features = ["serde"] }
```

<br>

## Examples

Three runnable examples live in [`examples/`](./examples):

```bash
# Assemble a function at run time, flip it to executable, and call it (x86-64).
cargo run --example jit_function

# Allocate a guard-flanked region and watch an overrun get rejected.
cargo run --example guard_pages

# Walk a region through the W^X lifecycle and watch the safe accessors track it.
cargo run --example protections
```

<br>

## Testing

```bash
# Unit, integration (workflow), property, serde, and doc tests
cargo test --all-features

# Property tests only
cargo test --all-features --test properties

# Lints and formatting
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings

# no_std build
cargo build --no-default-features

# Benchmarks (Criterion)
cargo bench --bench bench --all-features
```

The integration suite in [`tests/workflow.rs`](./tests/workflow.rs) walks the full lifecycle and, on x86-64, assembles a tiny function, flips the region to executable, and calls it through a transmuted function pointer &mdash; the same path a JIT takes. The property tests in [`tests/properties.rs`](./tests/properties.rs) check the size, bounds, and protection invariants across a wide range of inputs.

<br>

## Cross-Platform Support

Regions are backed by `mmap` / `mprotect` / `munmap` on Unix and `VirtualAlloc` / `VirtualProtect` / `VirtualFree` on Windows, reached through direct FFI. Linux, macOS, and Windows (x86-64 and ARM64) are the supported targets and are exercised by the CI matrix on stable and the 1.85 MSRV.

Getting memory into an executable state is portable; *running* freshly written code is not entirely. On architectures with separate instruction and data caches &mdash; AArch64, for instance &mdash; code written into a region must have the instruction cache synchronized before it is executed. On x86 and x86-64 this is automatic. This crate manages the memory and its protections; instruction-cache synchronization for the architectures that need it is the caller's responsibility at the point of execution.

<br>

## Safety

The crate uses `unsafe` where it must &mdash; FFI to the OS memory calls and the raw-pointer work behind the safe accessors. Every `unsafe` block carries a `// SAFETY:` justification, every `unsafe fn` documents its invariants, and the operations are covered by tests that map, write, protect, free, and execute real code on both Linux and Windows. The safe surface is designed so that no safe call can produce a dangling or wrongly-protected slice; the only inherently unsafe step left to the caller is transmuting a pointer to executable memory into a function and calling it.

<br>

## Contributing

Engineering standards for this crate are the [Rust Efficiency &amp; Performance Standards](./REPS.md); the current scope and plan are in [`dev/ROADMAP.md`](./dev/ROADMAP.md). Before a PR: `cargo fmt --all`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --all-features` must be clean.

<br>

<div id="license">
    <h2>License</h2>
    <p>Licensed under either of</p>
    <ul>
        <li><b>Apache License, Version 2.0</b> &mdash; <a href="./LICENSE-APACHE">LICENSE-APACHE</a></li>
        <li><b>MIT License</b> &mdash; <a href="./LICENSE-MIT">LICENSE-MIT</a></li>
    </ul>
    <p>at your option.</p>
</div>

<div align="center">
  <h2></h2>
  <sup>COPYRIGHT <small>&copy;</small> 2026 <strong>James Gober <me@jamesgober.com>.</strong></sup>
</div>
