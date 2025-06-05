use crate::{lib::std::mem, DeserializeError};

/// Metadata header which holds an ABI version and the length of the remaining
/// metadata.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MetadataHeader {
    magic: [u8; 8],
    version: u32,
    len: u32,
}

impl MetadataHeader {
    /// Current ABI version. Increment this any time breaking changes are made
    /// to the format of the serialized data.
    pub const CURRENT_VERSION: u32 = 10;

    /// The size of pointers used for references in the serialized data, in bits.
    ///
    /// Can be controlled through the `pointer_width_*` features in the rkyv
    /// crate, and MUST be included in cache keys to prevent trying to handle
    /// invalid artifacts.
    ///
    /// Normal values: 16/32/64.
    //
    // rkvy does not provide a way to get the pointer width directly.
    // This little trick works because the `FixedIsize` type is the size of a pointer.
    pub const POINTER_WIDTH_BITS: usize = core::mem::size_of::<rkyv::primitive::FixedIsize>() * 8;

    /// The default bit size of pointers used if no specific pointer width
    /// features are enabled in the rkyv crate.
    ///
    /// See [`Self::POINTER_WIDTH_BITS`].
    pub const POINTER_WIDTH_BITS_DEFAULT: usize = 32;

    /// Magic number to identify wasmer metadata.
    const MAGIC: [u8; 8] = *b"WASMER\0\0";

    /// Length of the metadata header.
    pub const LEN: usize = 16;

    /// Alignment of the metadata.
    pub const ALIGN: usize = 16;

    /// Creates a new header for metadata of the given length.
    pub fn new(len: usize) -> Self {
        Self {
            magic: Self::MAGIC,
            version: Self::CURRENT_VERSION,
            len: len.try_into().expect("metadata exceeds maximum length"),
        }
    }

    /// Convert the header into its bytes representation.
    pub fn into_bytes(self) -> [u8; 16] {
        unsafe { mem::transmute(self) }
    }

    /// Parses the header and returns the length of the metadata following it.
    pub fn parse(bytes: &[u8]) -> Result<usize, DeserializeError> {
        if bytes.as_ptr() as usize % 8 != 0 {
            return Err(DeserializeError::CorruptedBinary(
                "misaligned metadata".to_string(),
            ));
        }
        let bytes: [u8; 16] = bytes
            .get(..16)
            .ok_or_else(|| {
                DeserializeError::CorruptedBinary("invalid metadata header".to_string())
            })?
            .try_into()
            .unwrap();
        let header: Self = unsafe { mem::transmute(bytes) };
        if header.magic != Self::MAGIC {
            return Err(DeserializeError::Incompatible(
                "The provided bytes were not serialized by Wasmer".to_string(),
            ));
        }
        if header.version != Self::CURRENT_VERSION {
            return Err(DeserializeError::Incompatible(
                "The provided bytes were serialized by an incompatible version of Wasmer"
                    .to_string(),
            ));
        }
        Ok(header.len as usize)
    }
}
