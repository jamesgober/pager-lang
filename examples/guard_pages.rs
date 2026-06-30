//! Allocate a guard-flanked region and use it.
//!
//! A guarded region is bracketed by inaccessible pages, so a stray read or write that runs
//! off either end faults at once instead of quietly clobbering neighbouring memory. The
//! usable middle behaves exactly like a plain region. This example fills the region to its
//! last byte and shows that one byte further is rejected by the bounds check — long before
//! any access could reach the guard page itself.
//!
//! Run it with:
//!
//! ```bash
//! cargo run --example guard_pages
//! ```

use pager_lang::Region;

fn main() {
    let page = pager_lang::page_size();
    println!("host page size: {page} bytes");

    // Ask for 100 bytes; get a full page of usable space between two guard pages.
    let mut region = Region::with_guard_pages(100).expect("map guarded region");
    println!(
        "requested 100 bytes -> {} usable bytes, guard pages: {}",
        region.len(),
        region.has_guard_pages()
    );

    // Fill every usable byte, right up to the last one.
    let len = region.len();
    let filled = vec![0xA5u8; len];
    region.write(0, &filled).expect("fill region");
    println!("filled all {len} bytes");

    // Writing one byte past the end is caught by the bounds check.
    match region.write(len, &[0x00]) {
        Ok(()) => println!("unexpected: write past the end succeeded"),
        Err(e) => println!("write at offset {len} rejected: {e}"),
    }

    // The first and last usable bytes are exactly what we wrote.
    let bytes = region.as_slice().expect("readable");
    println!(
        "first byte = 0x{:02X}, last byte = 0x{:02X}",
        bytes[0],
        bytes[len - 1]
    );
}
