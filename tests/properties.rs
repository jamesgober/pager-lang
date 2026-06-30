//! Property tests for the invariants every region must hold, across a wide range of sizes,
//! offsets, and write lengths.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests assert on specific outcomes; a wrong outcome should fail the test loudly"
)]

use pager_lang::{PagerError, Protection, Region, page_size};
use proptest::prelude::*;

proptest! {
    /// Whatever size is asked for, the region is a whole number of pages and never smaller
    /// than the request.
    #[test]
    fn prop_length_is_page_rounded_and_at_least_requested(len in 1usize..=2_000_000) {
        let region = Region::new(len).unwrap();
        let page = page_size();
        prop_assert!(region.len() >= len);
        prop_assert_eq!(region.len() % page, 0);
        // Rounded up, not over: the request fits in the region but not with a spare page.
        prop_assert!(region.len() - len < page);
    }

    /// Any write that fits round-trips through `as_slice` byte for byte.
    #[test]
    fn prop_in_bounds_write_round_trips(
        offset in 0usize..4096,
        data in proptest::collection::vec(any::<u8>(), 0..4096),
    ) {
        // Four pages give room for any (offset, data) the strategy produces.
        let mut region = Region::new(4 * page_size()).unwrap();
        prop_assume!(offset + data.len() <= region.len());

        region.write(offset, &data).unwrap();
        prop_assert_eq!(&region.as_slice().unwrap()[offset..offset + data.len()], &data[..]);
    }

    /// A write whose end exceeds the region is always rejected, never truncated or wrapped.
    #[test]
    fn prop_out_of_bounds_write_is_rejected(extra in 1usize..4096) {
        let mut region = Region::new(page_size()).unwrap();
        let len = region.len();
        let result = region.write(len, &vec![0u8; extra]);
        let rejected = matches!(result, Err(PagerError::OutOfBounds { .. }));
        prop_assert!(rejected, "expected OutOfBounds, got {result:?}");
    }

    /// Cycling a region through every protection and back leaves it writable and intact,
    /// and the slice accessors track the current protection at each step.
    #[test]
    fn prop_protection_cycle_preserves_data(seed in any::<u8>()) {
        let mut region = Region::new(page_size()).unwrap();
        region.write(0, &[seed; 32]).unwrap();

        for prot in [
            Protection::Read,
            Protection::ReadExecute,
            Protection::None,
            Protection::ReadWrite,
        ] {
            region.protect(prot).unwrap();
            prop_assert_eq!(region.protection(), prot);
            prop_assert_eq!(region.as_slice().is_some(), prot.is_readable());
            prop_assert_eq!(region.as_mut_slice().is_some(), prot.is_writable());
        }

        // Ended writable; the original bytes survived the round trip.
        prop_assert_eq!(&region.as_slice().unwrap()[..32], &[seed; 32]);
    }

    /// Mapping and immediately dropping many regions neither fails nor leaks address space.
    #[test]
    fn prop_alloc_free_churn_is_stable(sizes in proptest::collection::vec(1usize..65_536, 1..64)) {
        for size in sizes {
            let region = Region::with_guard_pages(size).unwrap();
            prop_assert!(region.len() >= size);
            prop_assert!(region.has_guard_pages());
        }
    }
}
