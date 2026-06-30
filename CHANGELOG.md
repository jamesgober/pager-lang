<h1 align="center">
    <img width="90px" height="auto" src="https://raw.githubusercontent.com/jamesgober/jamesgober/main/media/icons/hexagon-3.svg" alt="Triple Hexagon">
    <br><b>CHANGELOG</b>
</h1>
<p>
  All notable changes to <code>pager-lang</code> will be documented in this file. The format is based on <a href="https://keepachangelog.com/en/1.1.0/">Keep a Changelog</a>,
  and this project adheres to <a href="https://semver.org/spec/v2.0.0.html/">Semantic Versioning</a>.
</p>

---

## [Unreleased]

### Added

### Changed

### Fixed

### Security

---

## [0.2.0] - 2026-06-30

The Core milestone: the virtual and executable memory layer lands. This is the first release with domain logic — it maps page-aligned regions, changes their protection, and frees them on drop, the foundation a JIT needs. The public surface is documented in [`docs/API.md`](docs/API.md) and remains pre-1.0 (subject to change until the `1.0.0` freeze).

### Added

- `Region` — an owned, page-aligned virtual-memory region, freed on drop. Created read/write with `new` or `with_guard_pages`; supports `write`, `protect`, `as_slice` / `as_mut_slice` (gated by current protection), raw `as_ptr` / `as_mut_ptr`, and `len` / `protection` / `has_guard_pages` accessors. `Send` + `Sync` + `Debug`.
- `Protection` — the access a region permits (`None`, `Read`, `ReadWrite`, `ReadExecute`, `ReadWriteExecute`) with `is_readable` / `is_writable` / `is_executable`.
- `PagerError` — the failure type: `ZeroSize`, `SizeOverflow`, `Map`, `Protect`, `NotWritable`, `OutOfBounds`. Carries the OS error code for OS-level failures; implements `Display`, `Error`, and is `#[non_exhaustive]`.
- `page_size` — the host page size, queried once and cached.
- Platform backends behind a thin internal layer: `mmap` / `mprotect` / `munmap` on Unix, `VirtualAlloc` / `VirtualProtect` / `VirtualFree` on Windows, via direct FFI with no third-party dependencies; guard pages implemented as inaccessible flanking pages on both.
- `serde` derives for `Protection` and `PagerError` behind the `serde` feature.
- Integration tests covering the full lifecycle, protection transitions, and guard-page bounds, including x86-64 tests that assemble a function, flip the region to executable, and run it through a transmuted function pointer on both the System V and Windows x64 calling conventions.
- Property tests for the size, bounds, and protection invariants; a `serde` round-trip test.
- Criterion benchmarks for mapping, protection changes, and writes across a range of sizes.
- Three runnable examples (`jit_function`, `guard_pages`, `protections`) and full rustdoc with runnable examples on every public item; `docs/API.md` API reference.

### Changed

- Replaced the crate's `#![forbid(unsafe_code)]` with scoped `unsafe` (FFI and raw-pointer work), every block carrying a `// SAFETY:` justification and every `unsafe fn` a `# Safety` section, under `#![deny(unsafe_op_in_unsafe_fn)]` and `#![deny(clippy::undocumented_unsafe_blocks)]`.

### Fixed

- Corrected the unparseable `keywords` and `categories` arrays in `Cargo.toml` that prevented the crate from building.
- Aligned `clippy.toml`'s `msrv` (`1.87` → `1.85`) with the crate's declared `rust-version`.
- Corrected the project name in the `deny.toml` header comment.
- Removed the README's reference to a `dev/DIRECTIVES.md` file that is not part of the repository.

---

## [0.1.0] - 2026-06-18

Initial scaffold and repository bootstrap. No domain logic yet &mdash; this release establishes the structure, tooling, and quality gates the implementation will be built on.

### Added

- `Cargo.toml` with crate metadata, Rust 2024 edition, MSRV 1.85.
- Dual `Apache-2.0 OR MIT` license files.
- `README.md`, `CHANGELOG.md`, and a documentation skeleton.
- `REPS.md` compliance baseline.
- `.github/workflows/ci.yml` CI matrix; `deny.toml`, `clippy.toml`, `rustfmt.toml`.
- `dev/DIRECTIVES.md` and `dev/ROADMAP.md` (committed engineering standards + plan).

[Unreleased]: https://github.com/jamesgober/pager-lang/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/jamesgober/pager-lang/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/jamesgober/pager-lang/releases/tag/v0.1.0
