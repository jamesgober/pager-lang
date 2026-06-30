//! Access permissions for a region of memory.

/// The access a page range permits: which of read, write, and execute the CPU will allow.
///
/// A [`Region`](crate::Region) is created read/write so code or data can be written into it,
/// then moved to another protection with [`Region::protect`](crate::Region::protect). The
/// pattern a JIT follows is *write xor execute* (W^X): fill the region while it is
/// [`ReadWrite`](Self::ReadWrite), then flip it to [`ReadExecute`](Self::ReadExecute) before
/// running the code. Memory that is writable and executable at the same time is a standing
/// invitation to turn a stray write into code execution, so [`ReadWriteExecute`](Self::ReadWriteExecute)
/// exists for the rare cases that need it but should be avoided where the two-step flip works.
///
/// The five cases map directly onto the platform primitives: `PROT_*` bits for `mprotect`
/// on Unix, `PAGE_*` constants for `VirtualProtect` on Windows.
///
/// # Examples
///
/// ```
/// use pager_lang::Protection;
///
/// assert!(Protection::ReadWrite.is_writable());
/// assert!(!Protection::ReadWrite.is_executable());
///
/// assert!(Protection::ReadExecute.is_executable());
/// assert!(!Protection::ReadExecute.is_writable());
///
/// // A guard page allows nothing at all.
/// assert!(!Protection::None.is_readable());
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Protection {
    /// No access. Any read, write, or execute through the range faults. This is what guard
    /// pages carry.
    None,
    /// Read-only. Suitable for constant data that must not change once written.
    Read,
    /// Read and write — the protection a freshly created [`Region`](crate::Region) starts
    /// with, so machine code or data can be written into it.
    ReadWrite,
    /// Read and execute. The target for finished JIT code: readable and runnable, but no
    /// longer writable, completing the W^X flip.
    ReadExecute,
    /// Read, write, and execute simultaneously. Convenient but it defeats W^X; prefer
    /// writing under [`ReadWrite`](Self::ReadWrite) and flipping to
    /// [`ReadExecute`](Self::ReadExecute). Some hardened platforms refuse this protection
    /// outright.
    ReadWriteExecute,
}

impl Protection {
    /// Whether reads are permitted. True for every protection except [`None`](Self::None).
    ///
    /// # Examples
    ///
    /// ```
    /// use pager_lang::Protection;
    ///
    /// assert!(Protection::Read.is_readable());
    /// assert!(Protection::ReadExecute.is_readable());
    /// assert!(!Protection::None.is_readable());
    /// ```
    #[must_use]
    pub const fn is_readable(self) -> bool {
        !matches!(self, Protection::None)
    }

    /// Whether writes are permitted. True for [`ReadWrite`](Self::ReadWrite) and
    /// [`ReadWriteExecute`](Self::ReadWriteExecute).
    ///
    /// [`Region::write`](crate::Region::write) and
    /// [`Region::as_mut_slice`](crate::Region::as_mut_slice) succeed exactly when the
    /// region's protection reports this true.
    ///
    /// # Examples
    ///
    /// ```
    /// use pager_lang::Protection;
    ///
    /// assert!(Protection::ReadWrite.is_writable());
    /// assert!(!Protection::ReadExecute.is_writable());
    /// ```
    #[must_use]
    pub const fn is_writable(self) -> bool {
        matches!(self, Protection::ReadWrite | Protection::ReadWriteExecute)
    }

    /// Whether execution is permitted. True for [`ReadExecute`](Self::ReadExecute) and
    /// [`ReadWriteExecute`](Self::ReadWriteExecute).
    ///
    /// # Examples
    ///
    /// ```
    /// use pager_lang::Protection;
    ///
    /// assert!(Protection::ReadExecute.is_executable());
    /// assert!(!Protection::ReadWrite.is_executable());
    /// ```
    #[must_use]
    pub const fn is_executable(self) -> bool {
        matches!(self, Protection::ReadExecute | Protection::ReadWriteExecute)
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests assert on specific outcomes; a wrong outcome should fail the test loudly"
)]
mod tests {
    use super::Protection;

    #[test]
    fn test_is_readable_true_for_all_but_none() {
        assert!(!Protection::None.is_readable());
        for prot in [
            Protection::Read,
            Protection::ReadWrite,
            Protection::ReadExecute,
            Protection::ReadWriteExecute,
        ] {
            assert!(prot.is_readable(), "{prot:?} should be readable");
        }
    }

    #[test]
    fn test_is_writable_only_for_write_protections() {
        assert!(Protection::ReadWrite.is_writable());
        assert!(Protection::ReadWriteExecute.is_writable());
        assert!(!Protection::None.is_writable());
        assert!(!Protection::Read.is_writable());
        assert!(!Protection::ReadExecute.is_writable());
    }

    #[test]
    fn test_is_executable_only_for_execute_protections() {
        assert!(Protection::ReadExecute.is_executable());
        assert!(Protection::ReadWriteExecute.is_executable());
        assert!(!Protection::None.is_executable());
        assert!(!Protection::Read.is_executable());
        assert!(!Protection::ReadWrite.is_executable());
    }
}
