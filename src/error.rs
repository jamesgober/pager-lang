//! The error a memory operation reports when it cannot complete.

use core::fmt;

/// The reason a [`Region`](crate::Region) operation failed.
///
/// The variants separate the three places things go wrong: a request that does not describe
/// a valid region ([`ZeroSize`](Self::ZeroSize), [`SizeOverflow`](Self::SizeOverflow)), the
/// operating system refusing a call ([`Map`](Self::Map), [`Protect`](Self::Protect)), and a
/// write that does not fit the region as it currently stands ([`NotWritable`](Self::NotWritable),
/// [`OutOfBounds`](Self::OutOfBounds)). The OS-level variants carry the raw error code —
/// `errno` on Unix, `GetLastError` on Windows — so the underlying cause is not lost.
///
/// The set is `#[non_exhaustive]`: future platforms or features may add variants, so a
/// `match` on this type must include a wildcard arm.
///
/// # Examples
///
/// ```
/// use pager_lang::{PagerError, Region};
///
/// // A region must be at least one page; zero bytes is rejected up front.
/// assert!(matches!(Region::new(0), Err(PagerError::ZeroSize)));
/// ```
#[derive(Clone, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum PagerError {
    /// A region of zero bytes was requested. A region must span at least one page; ask for
    /// the number of bytes you need and the request is rounded up to whole pages.
    ZeroSize,
    /// Rounding the request up to whole pages, or adding the guard pages, overflowed
    /// `usize`. The requested size is too large to represent, let alone map.
    SizeOverflow,
    /// The operating system refused to map the region. The wrapped value is the OS error
    /// code (`errno` / `GetLastError`); the usual cause is exhausted address space or
    /// memory.
    Map(i32),
    /// The operating system refused to change the region's protection. The wrapped value is
    /// the OS error code. A frequent cause is a platform that forbids writable-and-executable
    /// pages, which rejects [`Protection::ReadWriteExecute`](crate::Protection::ReadWriteExecute).
    Protect(i32),
    /// A write was attempted on a region whose current protection does not permit writing.
    /// Flip it to [`Protection::ReadWrite`](crate::Protection::ReadWrite) with
    /// [`Region::protect`](crate::Region::protect) first.
    NotWritable,
    /// A write would have fallen outside the region: `offset + len` exceeded the region's
    /// length. The fields report what was attempted against the region that exists.
    OutOfBounds {
        /// The offset, in bytes, the write started at.
        offset: usize,
        /// The number of bytes the write would have copied.
        len: usize,
        /// The length of the region, in bytes.
        region_len: usize,
    },
}

impl fmt::Display for PagerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PagerError::ZeroSize => f.write_str("cannot map a region of zero bytes"),
            PagerError::SizeOverflow => {
                f.write_str("region size overflowed usize when rounded to whole pages")
            }
            PagerError::Map(code) => {
                write!(
                    f,
                    "the operating system could not map the region (os error {code})"
                )
            }
            PagerError::Protect(code) => write!(
                f,
                "the operating system could not change the region's protection (os error {code})"
            ),
            PagerError::NotWritable => {
                f.write_str("the region is not writable under its current protection")
            }
            PagerError::OutOfBounds {
                offset,
                len,
                region_len,
            } => write!(
                f,
                "a write of {len} byte(s) at offset {offset} exceeds the {region_len}-byte region"
            ),
        }
    }
}

impl core::error::Error for PagerError {}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests assert on specific outcomes; a wrong outcome should fail the test loudly"
)]
mod tests {
    use super::PagerError;

    #[test]
    fn test_display_mentions_the_failure() {
        assert!(PagerError::ZeroSize.to_string().contains("zero"));
        assert!(PagerError::SizeOverflow.to_string().contains("overflow"));
        assert!(PagerError::Map(12).to_string().contains("os error 12"));
        assert!(PagerError::Protect(13).to_string().contains("os error 13"));
        assert!(PagerError::NotWritable.to_string().contains("not writable"));
        let oob = PagerError::OutOfBounds {
            offset: 8,
            len: 16,
            region_len: 16,
        };
        let text = oob.to_string();
        assert!(text.contains('8') && text.contains("16"));
    }

    #[test]
    fn test_is_a_std_error() {
        fn assert_error<E: core::error::Error>(_: &E) {}
        assert_error(&PagerError::ZeroSize);
    }
}
