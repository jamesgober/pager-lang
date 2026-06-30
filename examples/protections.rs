//! Walk a region through the write-xor-execute lifecycle and watch the safe accessors track
//! its protection at every step.
//!
//! Run it with:
//!
//! ```bash
//! cargo run --example protections
//! ```

use pager_lang::{Protection, Region};

fn main() {
    // A region is born read+write so it can be filled.
    let mut region = Region::new(4096).expect("map region");
    report(&region);

    // Write some bytes while it is writable.
    region.write(0, &[0xC3]).expect("write");
    println!(
        "wrote 1 byte; first byte is now 0x{:02X}",
        region.as_slice().unwrap()[0]
    );

    // Flip to read+execute: the write path closes, the execute path opens.
    region.protect(Protection::ReadExecute).expect("flip to rx");
    report(&region);
    println!(
        "  as_mut_slice() -> {:?}",
        region.as_mut_slice().map(|_| "Some")
    );
    println!("  write() -> {:?}", region.write(0, &[0]).err());

    // Lock it down completely: a guard-like no-access state.
    region.protect(Protection::None).expect("flip to none");
    report(&region);
    println!("  as_slice() -> {:?}", region.as_slice().map(|_| "Some"));

    // Reopen for writing; the bytes are still there.
    region
        .protect(Protection::ReadWrite)
        .expect("flip back to rw");
    report(&region);
    println!(
        "first byte survived: 0x{:02X}",
        region.as_slice().unwrap()[0]
    );
}

fn report(region: &Region) {
    let p = region.protection();
    println!(
        "protection {:?}: read={} write={} execute={}",
        p,
        p.is_readable(),
        p.is_writable(),
        p.is_executable()
    );
}
