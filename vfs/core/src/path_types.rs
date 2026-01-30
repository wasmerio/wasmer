//! Byte-oriented POSIX path types.
//!
//! These are `std::path`-like types, but backed by raw bytes to support WASI paths and
//! non-UTF-8 data. No normalization is performed here; semantic resolution is implemented
//! later by the path walker (Phase 2.1).

use crate::{VfsConfig, VfsError, VfsErrorKind, VfsResult};
use std::borrow::{Borrow, Cow};
use std::fmt;
use std::ops::Deref;

/// Borrowed VFS path, backed by raw bytes.
///
/// This is analogous to `std::path::Path`, but uses a fixed POSIX separator (`/`) and does not
/// interpret bytes as platform-specific encodings.
#[repr(transparent)]
pub struct VfsPath {
    inner: [u8],
}

impl VfsPath {
    /// Create a `&VfsPath` view over raw bytes.
    ///
    /// This does not validate bytes (e.g. NUL). Validate at syscall boundaries via
    /// [`VfsPath::validate`] and then pass `&VfsPath` through hot paths without rescanning.
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
    pub fn to_vec(&self) -> Vec<u8> {
        self.inner.to_vec()
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
    pub fn has_trailing_slash(&self) -> bool {
        self.inner.last().copied() == Some(b'/')
    }

    /// Root check that treats any non-empty all-slash sequence as root.
    #[inline]
    pub fn is_root_raw(&self) -> bool {
        !self.inner.is_empty() && self.inner.iter().all(|b| *b == b'/')
    }

    #[inline]
    pub fn is_root_canonical(&self) -> bool {
        self.inner == [b'/']
    }

    #[inline]
    pub fn to_path_buf(&self) -> VfsPathBuf {
        VfsPathBuf {
            inner: self.inner.to_vec(),
        }
    }

    #[inline]
    pub fn to_utf8(&self) -> Option<&str> {
        std::str::from_utf8(self.as_bytes()).ok()
    }

    #[inline]
    pub fn to_utf8_lossy(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(self.as_bytes())
    }

    /// Validate path bytes against config limits.
    ///
    /// Rejects:
    /// - embedded NUL bytes
    /// - length > `cfg.max_path_len`
    pub fn validate(&self, cfg: &VfsConfig) -> VfsResult<()> {
        if self.inner.len() > cfg.max_path_len {
            return Err(VfsError::new(VfsErrorKind::InvalidInput, "path.validate"));
        }
        if self.inner.iter().any(|b| *b == 0) {
            return Err(VfsError::new(VfsErrorKind::InvalidInput, "path.validate"));
        }
        Ok(())
    }

    #[inline]
    pub fn components(&self) -> VfsComponents<'_> {
        VfsComponents::new(self)
    }

    /// Iterate only normal (non `.`/`..`) components, skipping repeated slashes.
    #[inline]
    pub fn normal_components(&self) -> impl Iterator<Item = &[u8]> {
        self.components().filter_map(|c| match c {
            VfsComponent::Normal(seg) => Some(seg),
            _ => None,
        })
    }
}

impl fmt::Debug for VfsPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("VfsPath(")?;
        fmt_bytes_as_debug_string(f, self.as_bytes())?;
        f.write_str(")")
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

impl<'a> From<&'a [u8]> for &'a VfsPath {
    #[inline]
    fn from(value: &'a [u8]) -> Self {
        VfsPath::new(value)
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
        Self { inner: bytes.into() }
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

    /// Join `seg` onto this path.
    ///
    /// - If `seg` is absolute, replaces `self` entirely.
    /// - If `seg` is relative, appends it, ensuring exactly one `/` separator.
    /// - Leading slashes in a relative `seg` are stripped to avoid "silent absolute injection".
    pub fn push(&mut self, seg: &VfsPath) {
        if seg.is_absolute() {
            self.inner.clear();
            self.inner.extend_from_slice(seg.as_bytes());
            return;
        }

        let mut seg_bytes = seg.as_bytes();
        while seg_bytes.first().copied() == Some(b'/') {
            seg_bytes = &seg_bytes[1..];
        }
        if seg_bytes.is_empty() {
            return;
        }

        if !self.inner.is_empty() && !self.inner.ends_with(b"/") {
            self.inner.push(b'/');
        }
        self.inner.extend_from_slice(seg_bytes);
    }

    /// Push a single validated name component.
    pub fn push_name(&mut self, name: VfsName<'_>) {
        if !self.inner.is_empty() && !self.inner.ends_with(b"/") {
            self.inner.push(b'/');
        }
        self.inner.extend_from_slice(name.as_bytes());
    }

    /// Raw concatenation without join semantics.
    pub fn push_raw(&mut self, seg: &VfsPath) {
        self.inner.extend_from_slice(seg.as_bytes());
    }

    /// Pop the last non-empty component (lexical; does not resolve symlinks).
    pub fn pop(&mut self) -> bool {
        if self.inner.is_empty() {
            return false;
        }

        while self.inner.len() > 1 && self.inner.ends_with(b"/") {
            self.inner.pop();
        }

        if self.inner == [b'/'] {
            return false;
        }

        if let Some(pos) = self.inner.iter().rposition(|b| *b == b'/') {
            if pos == 0 {
                self.inner.truncate(1);
            } else {
                self.inner.truncate(pos);
            }
            true
        } else {
            self.inner.clear();
            true
        }
    }
}

impl fmt::Debug for VfsPathBuf {
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

/// A validated single path component (a name).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VfsName<'a>(&'a [u8]);

impl<'a> VfsName<'a> {
    pub fn new(bytes: &'a [u8]) -> VfsResult<Self> {
        if bytes.is_empty() {
            return Err(VfsError::new(VfsErrorKind::InvalidInput, "name.validate"));
        }
        if bytes.iter().any(|b| *b == 0 || *b == b'/') {
            return Err(VfsError::new(VfsErrorKind::InvalidInput, "name.validate"));
        }
        Ok(Self(bytes))
    }

    #[inline]
    pub fn as_bytes(self) -> &'a [u8] {
        self.0
    }
}

/// Owned validated name component.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct VfsNameBuf(Vec<u8>);

impl VfsNameBuf {
    pub fn new(bytes: Vec<u8>) -> VfsResult<Self> {
        VfsName::new(&bytes)?;
        Ok(Self(bytes))
    }

    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for VfsNameBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt_bytes_as_debug_string(f, self.as_bytes())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VfsComponent<'a> {
    RootDir,
    CurDir,
    ParentDir,
    Normal(&'a [u8]),
}

pub struct VfsComponents<'a> {
    bytes: &'a [u8],
    pos: usize,
    emitted_root: bool,
    absolute: bool,
}

impl<'a> VfsComponents<'a> {
    fn new(path: &'a VfsPath) -> Self {
        let bytes = path.as_bytes();
        let absolute = bytes.first().copied() == Some(b'/');
        Self {
            bytes,
            pos: 0,
            emitted_root: false,
            absolute,
        }
    }
}

impl<'a> Iterator for VfsComponents<'a> {
    type Item = VfsComponent<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.bytes.is_empty() {
            return None;
        }

        if self.absolute && !self.emitted_root {
            self.emitted_root = true;
            while self.pos < self.bytes.len() && self.bytes[self.pos] == b'/' {
                self.pos += 1;
            }
            return Some(VfsComponent::RootDir);
        }

        while self.pos < self.bytes.len() && self.bytes[self.pos] == b'/' {
            self.pos += 1;
        }
        if self.pos >= self.bytes.len() {
            return None;
        }

        let start = self.pos;
        while self.pos < self.bytes.len() && self.bytes[self.pos] != b'/' {
            self.pos += 1;
        }
        let seg = &self.bytes[start..self.pos];
        match seg {
            b"." => Some(VfsComponent::CurDir),
            b".." => Some(VfsComponent::ParentDir),
            _ => Some(VfsComponent::Normal(seg)),
        }
    }
}

fn fmt_bytes_as_debug_string(f: &mut fmt::Formatter<'_>, bytes: &[u8]) -> fmt::Result {
    f.write_str("b\"")?;
    for &b in bytes {
        match b {
            b'\\' => f.write_str("\\\\")?,
            b'"' => f.write_str("\\\"")?,
            b'\n' => f.write_str("\\n")?,
            b'\r' => f.write_str("\\r")?,
            b'\t' => f.write_str("\\t")?,
            0x20..=0x7E => f.write_str(std::str::from_utf8(&[b]).unwrap())?,
            _ => write!(f, "\\x{b:02x}")?,
        }
    }
    f.write_str("\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_utf8_roundtrip() {
        let p = VfsPath::new(b"/tmp/\xFF");
        assert_eq!(p.as_bytes(), b"/tmp/\xFF");
        let buf = p.to_path_buf();
        assert_eq!(buf.as_bytes(), b"/tmp/\xFF");
    }

    #[test]
    fn components_examples() {
        let cases: &[(&[u8], Vec<VfsComponent<'_>>)] = &[
            (b"", vec![]),
            (b"/", vec![VfsComponent::RootDir]),
            (
                b"//a///b/",
                vec![
                    VfsComponent::RootDir,
                    VfsComponent::Normal(b"a"),
                    VfsComponent::Normal(b"b"),
                ],
            ),
            (
                b"./a/../b",
                vec![
                    VfsComponent::CurDir,
                    VfsComponent::Normal(b"a"),
                    VfsComponent::ParentDir,
                    VfsComponent::Normal(b"b"),
                ],
            ),
        ];

        for (raw, expected) in cases {
            let got: Vec<_> = VfsPath::new(raw).components().collect();
            assert_eq!(&got, expected);
        }
    }

    #[test]
    fn validate_rejects_nul_and_too_long() {
        let cfg = VfsConfig {
            max_symlinks: 40,
            max_path_len: 4,
            max_name_len: 255,
        };

        assert!(VfsPath::new(b"a\0b").validate(&cfg).is_err());
        assert!(VfsPath::new(b"12345").validate(&cfg).is_err());
        assert!(VfsPath::new(b"1234").validate(&cfg).is_ok());
    }
}
