//! VFS-level capabilities exposed by an `Fs` instance.

/// Coarse feature/capability model used by the VFS core to gate semantics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VfsCapabilities(u64);

impl VfsCapabilities {
    pub const NONE: Self = Self(0);

    pub const SYMLINKS: Self = Self(1 << 0);
    pub const HARDLINKS: Self = Self(1 << 1);
    pub const CHMOD: Self = Self(1 << 2);
    pub const CHOWN: Self = Self(1 << 3);
    pub const UTIMENS: Self = Self(1 << 4);
    pub const RENAME_EXCHANGE: Self = Self(1 << 5);

    #[inline]
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    #[inline]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}
