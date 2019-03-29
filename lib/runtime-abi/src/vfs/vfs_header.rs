/// Represents the version of this header schema.
#[repr(u8)]
#[derive(Debug, PartialEq)]
pub enum HeaderVersion {
    Version1 = 1,
}

/// Represents the compression type of the file data. Only Zstd or no-compression is supported.
#[repr(u8)]
#[derive(Debug, PartialEq)]
pub enum CompressionType {
    NONE = 0,
    ZSTD = 1,
}

/// Represents the type of archive. The only supported archive is the Tar format.
#[repr(u8)]
#[derive(Debug, PartialEq)]
pub enum ArchiveType {
    TAR = 0,
}

// extract the header data from bytes
pub fn header_from_bytes(
    bytes: &[u8],
) -> Result<(HeaderVersion, CompressionType, ArchiveType), HeaderError> {
    if let Some(bytes) = bytes.get(..4) {
        let version = match bytes[0] {
            1 => HeaderVersion::Version1,
            x => return Err(HeaderError::UnknownHeaderVersion(x)),
        };
        let compression_type = match bytes[1] {
            0 => CompressionType::NONE,
            1 => CompressionType::ZSTD,
            x => return Err(HeaderError::UnknownCompressionType(x)),
        };
        let archive_type = match bytes[2] {
            0 => ArchiveType::TAR,
            x => return Err(HeaderError::UnknownArchiveType(x)),
        };
        Ok((version, compression_type, archive_type))
    } else {
        Err(HeaderError::HeaderTooSmall)
    }
}

#[derive(Debug, Fail)]
pub enum HeaderError {
    #[fail(display = "The version is not supported: \"{}\"", _0)]
    UnknownHeaderVersion(u8),
    #[fail(display = "The compression type is unknown: \"{}\"", _0)]
    UnknownCompressionType(u8),
    #[fail(display = "The archive type is unknown: \"{}\"", _0)]
    UnknownArchiveType(u8),
    #[fail(display = "The header is too small.")]
    HeaderTooSmall,
}
