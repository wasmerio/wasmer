//! Macros and abstractions used during testing.

use bytes::{Bytes, BytesMut};

use webc::Version;

/// Construct a sequence of bytes using one or more items that implement the
/// [`ToBytes`] trait.
macro_rules! bytes {
    ($($item:expr),* $(,)?) => {
        {
            #[allow(unused_mut)]
            let mut buffer: ::bytes::BytesMut = ::bytes::BytesMut::new();
            $(
                #[allow(clippy::identity_op)]
                $crate::macros::ToBytes::to_bytes(&$item, &mut buffer);
            )*
            buffer.freeze()
        }
    };
}

/// Write something to a byte buffer.
pub(crate) trait ToBytes {
    fn to_bytes(&self, buffer: &mut BytesMut);
}

impl ToBytes for webc::v3::Tag {
    fn to_bytes(&self, buffer: &mut BytesMut) {
        self.as_u8().to_bytes(buffer);
    }
}

impl ToBytes for webc::v3::Timestamps {
    fn to_bytes(&self, buffer: &mut BytesMut) {
        // TODO: This impl is used in the bytes macro. This macro
        // is not public, so this unwrap should be fine for now.
        // But a better solution will make `ToBytes` fallible.
        self.write_to(buffer).unwrap()
    }
}

impl ToBytes for [u8] {
    fn to_bytes(&self, buffer: &mut BytesMut) {
        buffer.extend_from_slice(self);
    }
}

impl<const N: usize> ToBytes for [u8; N] {
    fn to_bytes(&self, buffer: &mut BytesMut) {
        buffer.extend_from_slice(self);
    }
}

impl ToBytes for str {
    fn to_bytes(&self, buffer: &mut BytesMut) {
        self.as_bytes().to_bytes(buffer);
    }
}

impl<T: ToBytes + ?Sized> ToBytes for &T {
    fn to_bytes(&self, buffer: &mut BytesMut) {
        (**self).to_bytes(buffer);
    }
}

impl ToBytes for u8 {
    fn to_bytes(&self, buffer: &mut BytesMut) {
        [*self].to_bytes(buffer);
    }
}

impl ToBytes for Vec<u8> {
    fn to_bytes(&self, buffer: &mut BytesMut) {
        self.as_slice().to_bytes(buffer);
    }
}

impl ToBytes for Bytes {
    fn to_bytes(&self, buffer: &mut BytesMut) {
        self.as_ref().to_bytes(buffer);
    }
}

impl ToBytes for Version {
    fn to_bytes(&self, buffer: &mut BytesMut) {
        self.0.to_bytes(buffer);
    }
}

impl ToBytes for BytesMut {
    fn to_bytes(&self, buffer: &mut BytesMut) {
        self.as_ref().to_bytes(buffer);
    }
}

macro_rules! impl_to_bytes_le {
    ($( $type:ty ),* $(,)?) => {
        $(
            impl ToBytes for $type {
                fn to_bytes(&self, buffer: &mut BytesMut) {
                    self.to_le_bytes().to_bytes(buffer);
                }
            }
        )*
    };
}

impl_to_bytes_le!(u16, u32);

macro_rules! assert_bytes_eq {
    ($lhs:expr, $rhs:expr, $msg:literal $( $tokens:tt)*) => {{
        let lhs = &$lhs[..];
        let rhs = &$rhs[..];
        if lhs != rhs {
            let lhs: Vec<_> = hexdump::hexdump_iter(lhs)
                .map(|line| line.to_string())
                .collect();
            let rhs: Vec<_> = hexdump::hexdump_iter(rhs)
                .map(|line| line.to_string())
                .collect::<Vec<_>>();

            pretty_assertions::assert_eq!(lhs.join("\n"), rhs.join("\n"), $msg $($tokens)*);
        }
    }};
    ($lhs:expr, $rhs:expr) => {
        assert_bytes_eq!($lhs, $rhs, "{} != {}", stringify!($lhs), stringify!($rhs));
    };
}
