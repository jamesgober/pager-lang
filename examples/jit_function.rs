//! Compile a function at run time and call it.
//!
//! This is the whole point of the crate in miniature: assemble machine code into a region,
//! flip the region to read+execute, transmute its address to a function pointer, and call it.
//! The code is x86-64; on other architectures the example prints a note and exits, since the
//! byte sequence would not be valid instructions there.
//!
//! Run it with:
//!
//! ```bash
//! cargo run --example jit_function
//! ```

fn main() {
    #[cfg(target_arch = "x86_64")]
    run();

    #[cfg(not(target_arch = "x86_64"))]
    println!("this example assembles x86-64 code; nothing to run on this architecture");
}

#[cfg(target_arch = "x86_64")]
fn run() {
    use pager_lang::{Protection, Region};

    // fn add(a, b) -> a + b, for the host's C calling convention.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    // System V: first two integer args in rdi, rsi.
    //   mov rax, rdi ; add rax, rsi ; ret
    const CODE: [u8; 7] = [0x48, 0x89, 0xF8, 0x48, 0x01, 0xF0, 0xC3];

    #[cfg(target_os = "windows")]
    // Windows x64: first two integer args in rcx, rdx.
    //   mov rax, rcx ; add rax, rdx ; ret
    const CODE: [u8; 7] = [0x48, 0x89, 0xC8, 0x48, 0x01, 0xD0, 0xC3];

    // 1. Reserve a writable, guard-flanked region for the code.
    let mut region = Region::with_guard_pages(CODE.len()).expect("map code region");
    println!(
        "mapped {} bytes (guard pages: {})",
        region.len(),
        region.has_guard_pages()
    );

    // 2. Write the machine code.
    region.write(0, &CODE).expect("write code");

    // 3. W^X flip: make it read+execute. It is no longer writable.
    region
        .protect(Protection::ReadExecute)
        .expect("make executable");
    assert!(region.as_mut_slice().is_none());

    // 4. Run it.
    // SAFETY: the region holds a valid x86-64 function taking two integer arguments and
    // returning their sum in rax, matching `extern "C" fn(u64, u64) -> u64`; it is mapped
    // read+execute and x86-64 keeps instruction and data caches coherent.
    let add: extern "C" fn(u64, u64) -> u64 = unsafe { core::mem::transmute(region.as_ptr()) };
    let result = add(19, 23);
    println!("jitted add(19, 23) = {result}");
    assert_eq!(result, 42);
}
