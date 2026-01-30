//! VFS-native path types.
//!
//! `vfs-core` must not depend on `std::path::{Path, PathBuf}` because it needs to
//! handle raw bytes (non-UTF8) and keep platform-specific semantics out of the core.
//!
//! These types are intentionally minimal; normalization and mount-aware resolution
//! are implemented later (Phase 2.1).

use std::borrow::{Borrow, Cow};
use std::fmt;
use std::ops::Deref;

/// Borrowed VFS path, backed by raw bytes.
///
/// This is analogous to `std::path::Path`, but intentionally does not use platform
/// encodings. Consumers decide how/when to interpret these bytes.
#[repr(transparent)]
pub struct VfsPath {
    inner: [u8],
}

impl VfsPath {
    #[inline]
    pub fn new(bytes: &[u8]) -> &Self {
        // SAFETY: `VfsPath` is `repr(transparent)` over `[u8]`.
        unsafe { &*(bytes as *const [u8] as *const VfsPath) }
    }

    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        &self.inner
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    #[inline]
    pub fn is_absolute(&self) -> bool {
        self.inner.first().copied() == Some(b'/')
    }

    #[inline]
    pub fn to_path_buf(&self) -> VfsPathBuf {
        VfsPathBuf {
            inner: self.inner.to_vec(),
        }
    }

    #[inline]
    pub fn as_str_lossy(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(self.as_bytes())
    }
}

impl fmt::Debug for VfsPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("VfsPath")
            .field(&self.as_str_lossy())
            .finish()
    }
}

impl fmt::Display for VfsPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.as_str_lossy())
    }
}

impl ToOwned for VfsPath {
    type Owned = VfsPathBuf;

    #[inline]
    fn to_owned(&self) -> Self::Owned {
        self.to_path_buf()
    }
}

impl AsRef<VfsPath> for VfsPath {
    #[inline]
    fn as_ref(&self) -> &VfsPath {
        self
    }
}

/// Owned VFS path, backed by raw bytes.
#[derive(Clone, PartialEq, Eq, Hash, Default)]
pub struct VfsPathBuf {
    inner: Vec<u8>,
}

impl VfsPathBuf {
    #[inline]
    pub fn new() -> Self {
        Self { inner: Vec::new() }
    }

    #[inline]
    pub fn from_bytes(bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            inner: bytes.into(),
        }
    }

    #[inline]
    pub fn as_path(&self) -> &VfsPath {
        VfsPath::new(&self.inner)
    }

    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        &self.inner
    }

    #[inline]
    pub fn into_bytes(self) -> Vec<u8> {
        self.inner
    }
}

impl fmt::Debug for VfsPathBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_path().fmt(f)
    }
}

impl fmt::Display for VfsPathBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_path().fmt(f)
    }
}

impl Deref for VfsPathBuf {
    type Target = VfsPath;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.as_path()
    }
}

impl Borrow<VfsPath> for VfsPathBuf {
    #[inline]
    fn borrow(&self) -> &VfsPath {
        self.as_path()
    }
}

impl AsRef<VfsPath> for VfsPathBuf {
    #[inline]
    fn as_ref(&self) -> &VfsPath {
        self.as_path()
    }
}

impl From<&str> for VfsPathBuf {
    #[inline]
    fn from(value: &str) -> Self {
        Self::from_bytes(value.as_bytes().to_vec())
    }
}

impl From<String> for VfsPathBuf {
    #[inline]
    fn from(value: String) -> Self {
        Self::from_bytes(value.into_bytes())
    }
}

impl From<Vec<u8>> for VfsPathBuf {
    #[inline]
    fn from(value: Vec<u8>) -> Self {
        Self::from_bytes(value)
    }
}

impl From<&[u8]> for VfsPathBuf {
    #[inline]
    fn from(value: &[u8]) -> Self {
        Self::from_bytes(value.to_vec())
    }
}
