use bytes::Bytes;
use shared_buffer::OwnedBuffer;
use std::borrow::Cow;

/// Convert binary data into [`bytes::Bytes`].
///
/// `Bytes`, `Vec<u8>`, `Box<[u8]>`, `String` and in-memory `OwnedBuffer`s
/// convert without copying; other data, including anything borrowed, is
/// copied. The data stays readable as a slice through [`AsRef`], so APIs that
/// only read it can skip the conversion entirely.
pub trait IntoBytes: AsRef<[u8]> {
    /// Convert binary data into [`bytes::Bytes`].
    fn into_bytes(self) -> Bytes;
}

impl IntoBytes for Bytes {
    fn into_bytes(self) -> Bytes {
        self
    }
}

impl IntoBytes for Vec<u8> {
    fn into_bytes(self) -> Bytes {
        Bytes::from(self)
    }
}

impl IntoBytes for Box<[u8]> {
    fn into_bytes(self) -> Bytes {
        Bytes::from(self)
    }
}

impl IntoBytes for String {
    fn into_bytes(self) -> Bytes {
        Bytes::from(self)
    }
}

impl<const N: usize> IntoBytes for [u8; N] {
    fn into_bytes(self) -> Bytes {
        Bytes::from(self.to_vec())
    }
}

impl IntoBytes for Cow<'_, [u8]> {
    fn into_bytes(self) -> Bytes {
        Bytes::from(self.into_owned())
    }
}

impl IntoBytes for OwnedBuffer {
    /// Shares an in-memory buffer; copies a memory-mapped one.
    fn into_bytes(self) -> Bytes {
        OwnedBuffer::into_bytes(self)
    }
}

impl<T: AsRef<[u8]> + ?Sized> IntoBytes for &T {
    fn into_bytes(self) -> Bytes {
        Bytes::copy_from_slice(self.as_ref())
    }
}
