//! Throughput benchmarks for the region lifecycle.
//!
//! The cost drivers are the operating-system calls — mapping, changing protection, and
//! unmapping — plus the in-process byte copy a region is filled with. Each group isolates
//! one of those so a regression points at a specific operation rather than the whole flow.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use pager_lang::{Protection, Region};

/// Map a region and immediately drop it: one `mmap`/`VirtualAlloc` plus one
/// `munmap`/`VirtualFree`, across a range of sizes.
fn bench_alloc_free(c: &mut Criterion) {
    let mut group = c.benchmark_group("alloc_free");
    for &pages in &[1usize, 16, 256] {
        let bytes = pages * 4096;
        group.throughput(Throughput::Bytes(bytes as u64));
        group.bench_with_input(BenchmarkId::new("plain", pages), &bytes, |b, &bytes| {
            b.iter(|| {
                let region = Region::new(black_box(bytes)).expect("map");
                black_box(&region);
            });
        });
        group.bench_with_input(BenchmarkId::new("guarded", pages), &bytes, |b, &bytes| {
            b.iter(|| {
                let region = Region::with_guard_pages(black_box(bytes)).expect("map");
                black_box(&region);
            });
        });
    }
    group.finish();
}

/// The W^X flip and back: two `mprotect`/`VirtualProtect` calls on a live region.
fn bench_protect(c: &mut Criterion) {
    let mut group = c.benchmark_group("protect");
    for &pages in &[1usize, 16, 256] {
        let bytes = pages * 4096;
        group.throughput(Throughput::Bytes(bytes as u64));
        group.bench_with_input(BenchmarkId::new("rw_rx_rw", pages), &bytes, |b, &bytes| {
            let mut region = Region::new(bytes).expect("map");
            b.iter(|| {
                region
                    .protect(black_box(Protection::ReadExecute))
                    .expect("rx");
                region
                    .protect(black_box(Protection::ReadWrite))
                    .expect("rw");
            });
        });
    }
    group.finish();
}

/// Copying machine code into a region — the in-process part of the flow, no syscall.
fn bench_write(c: &mut Criterion) {
    let mut group = c.benchmark_group("write");
    for &len in &[64usize, 4096, 65_536] {
        let code = vec![0x90u8; len];
        let mut region = Region::new(len).expect("map");
        group.throughput(Throughput::Bytes(len as u64));
        group.bench_with_input(BenchmarkId::new("bytes", len), &code, |b, code| {
            b.iter(|| region.write(0, black_box(code)).expect("write"));
        });
    }
    group.finish();
}

criterion_group!(benches, bench_alloc_free, bench_protect, bench_write);
criterion_main!(benches);
