//! Serialization round-trips for the value types, behind the `serde` feature.

#![cfg(feature = "serde")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests assert on specific outcomes; a wrong outcome should fail the test loudly"
)]

use pager_lang::{PagerError, Protection};

#[test]
fn test_protection_round_trips_through_json() {
    for prot in [
        Protection::None,
        Protection::Read,
        Protection::ReadWrite,
        Protection::ReadExecute,
        Protection::ReadWriteExecute,
    ] {
        let json = serde_json::to_string(&prot).unwrap();
        let back: Protection = serde_json::from_str(&json).unwrap();
        assert_eq!(prot, back);
    }
}

#[test]
fn test_pager_error_round_trips_through_json() {
    let errors = [
        PagerError::ZeroSize,
        PagerError::SizeOverflow,
        PagerError::Map(12),
        PagerError::Protect(13),
        PagerError::NotWritable,
        PagerError::OutOfBounds {
            offset: 8,
            len: 16,
            region_len: 16,
        },
    ];
    for err in errors {
        let json = serde_json::to_string(&err).unwrap();
        let back: PagerError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, back);
    }
}
