//! End-to-end workflow tests.
//!
//! These walk the path a JIT or runtime actually takes: map a region, write into it, change
//! its protection, and (on x86-64) execute the bytes that were written. The architecture-
//! gated tests assemble a tiny function by hand, flip the region to read+execute, and call
//! it through a transmuted function pointer — the real reason the crate exists.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests assert on specific outcomes; a wrong outcome should fail the test loudly"
)]

use pager_lang::{PagerError, Protection, Region};

#[test]
fn test_full_lifecycle_map_write_protect_drop() {
    let mut region = Region::new(256).unwrap();
    assert_eq!(region.protection(), Protection::ReadWrite);
    assert!(region.len() >= 256);

    // Fill it, read it back.
    let pattern: Vec<u8> = (0..64).collect();
    region.write(0, &pattern).unwrap();
    assert_eq!(&region.as_slice().unwrap()[..64], &pattern[..]);

    // W^X flip: writable -> executable.
    region.protect(Protection::ReadExecute).unwrap();
    assert!(region.as_mut_slice().is_none());
    assert!(region.as_slice().is_some());

    // Back to writable to overwrite, proving the flip is reversible.
    region.protect(Protection::ReadWrite).unwrap();
    region.write(0, &[0xFF; 64]).unwrap();
    assert_eq!(&region.as_slice().unwrap()[..64], &[0xFF; 64]);
    // Dropped here without leaking.
}

#[test]
fn test_protection_transitions_gate_access_correctly() {
    let mut region = Region::new(64).unwrap();

    region.protect(Protection::Read).unwrap();
    assert!(region.as_slice().is_some());
    assert!(region.as_mut_slice().is_none());
    assert_eq!(region.write(0, &[1]), Err(PagerError::NotWritable));

    region.protect(Protection::None).unwrap();
    assert!(region.as_slice().is_none());
    assert!(region.as_mut_slice().is_none());

    region.protect(Protection::ReadWrite).unwrap();
    assert!(region.as_mut_slice().is_some());
}

#[test]
fn test_guarded_region_round_trips_data() {
    let mut region = Region::with_guard_pages(512).unwrap();
    assert!(region.has_guard_pages());

    let data: Vec<u8> = (0..=255).cycle().take(512).collect();
    region.write(0, &data).unwrap();
    assert_eq!(&region.as_slice().unwrap()[..512], &data[..]);

    // The end of the usable region is reachable; one past it is not.
    let len = region.len();
    region.write(len - 1, &[0x7E]).unwrap();
    assert!(matches!(
        region.write(len, &[0x7E]),
        Err(PagerError::OutOfBounds { .. })
    ));
}

#[test]
fn test_regions_are_independent() {
    let mut a = Region::new(64).unwrap();
    let mut b = Region::new(64).unwrap();
    a.write(0, &[0xAA; 8]).unwrap();
    b.write(0, &[0xBB; 8]).unwrap();
    assert_eq!(&a.as_slice().unwrap()[..8], &[0xAA; 8]);
    assert_eq!(&b.as_slice().unwrap()[..8], &[0xBB; 8]);
}

/// Compiles `fn() -> u64 { 42 }` into a region and runs it. The byte sequence is the same
/// on every x86-64 OS, and a no-argument function returning in `rax` follows both the
/// System V and Windows x64 conventions, so this one test runs on all three platforms.
#[test]
#[cfg(target_arch = "x86_64")]
fn test_x86_64_jit_constant_function_executes() {
    // mov eax, 42 ; ret   (writing eax zero-extends into rax)
    const CODE: [u8; 6] = [0xB8, 0x2A, 0x00, 0x00, 0x00, 0xC3];

    let mut region = Region::new(CODE.len()).unwrap();
    region.write(0, &CODE).unwrap();
    region.protect(Protection::ReadExecute).unwrap();

    // SAFETY: the region holds a valid x86-64 function that takes no arguments and returns
    // its result in rax, matching the `extern "C" fn() -> u64` signature; the region is
    // read+execute, and on x86-64 instruction and data caches are coherent.
    let func: extern "C" fn() -> u64 = unsafe { core::mem::transmute(region.as_ptr()) };
    assert_eq!(func(), 42);
}

/// Compiles `fn(x) -> x + x` and runs it. The argument register differs by calling
/// convention, so the code is selected per OS.
#[test]
#[cfg(all(target_arch = "x86_64", any(target_os = "linux", target_os = "macos")))]
fn test_x86_64_sysv_jit_doubles_argument() {
    // System V: first integer argument in rdi.
    // mov rax, rdi ; add rax, rdi ; ret
    const CODE: [u8; 7] = [0x48, 0x89, 0xF8, 0x48, 0x01, 0xF8, 0xC3];
    run_double_test(&CODE);
}

/// As above, for the Windows x64 calling convention.
#[test]
#[cfg(all(target_arch = "x86_64", target_os = "windows"))]
fn test_x86_64_win64_jit_doubles_argument() {
    // Windows x64: first integer argument in rcx.
    // mov rax, rcx ; add rax, rcx ; ret
    const CODE: [u8; 7] = [0x48, 0x89, 0xC8, 0x48, 0x01, 0xC8, 0xC3];
    run_double_test(&CODE);
}

#[cfg(target_arch = "x86_64")]
fn run_double_test(code: &[u8]) {
    let mut region = Region::with_guard_pages(code.len()).unwrap();
    region.write(0, code).unwrap();
    region.protect(Protection::ReadExecute).unwrap();

    // SAFETY: the region holds a valid x86-64 function taking one integer argument and
    // returning it doubled in rax, matching `extern "C" fn(u64) -> u64`; the region is
    // read+execute and x86-64 caches are coherent.
    let double: extern "C" fn(u64) -> u64 = unsafe { core::mem::transmute(region.as_ptr()) };
    assert_eq!(double(21), 42);
    assert_eq!(double(0), 0);
    assert_eq!(double(1_000), 2_000);
}
