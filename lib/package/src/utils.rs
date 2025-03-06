use bytes::{Buf, Bytes};
use std::{
    fs::File,
    io::{BufRead, BufReader, Read, Seek},
    path::Path,
};
use wasmer_types::Features;
use webc::{Container, ContainerError, Version};

use crate::package::{Package, WasmerPackageError};

/// Check if something looks like a `*.tar.gz` file.
fn is_tarball(mut file: impl Read + Seek) -> bool {
    /// Magic bytes for a `*.tar.gz` file according to
    /// [Wikipedia](https://en.wikipedia.org/wiki/List_of_file_signatures).
    const TAR_GZ_MAGIC_BYTES: [u8; 2] = [0x1F, 0x8B];

    let mut buffer = [0_u8; 2];
    let result = match file.read_exact(&mut buffer) {
        Ok(_) => buffer == TAR_GZ_MAGIC_BYTES,
        Err(_) => false,
    };

    let _ = file.rewind();

    result
}

pub fn from_disk(path: impl AsRef<Path>) -> Result<Container, WasmerPackageError> {
    let path = path.as_ref();

    if path.is_dir() {
        return parse_dir(path);
    }

    let mut f = File::open(path).map_err(|error| ContainerError::Open {
        error,
        path: path.to_path_buf(),
    })?;

    if is_tarball(&mut f) {
        return parse_tarball(BufReader::new(f));
    }

    match webc::detect(&mut f) {
        Ok(Version::V1) => parse_v1_mmap(f).map_err(Into::into),
        Ok(Version::V2) => parse_v2_mmap(f).map_err(Into::into),
        Ok(Version::V3) => parse_v3_mmap(f).map_err(Into::into),
        Ok(other) => {
            // fall back to the allocating generic version
            let mut buffer = Vec::new();
            f.rewind()
                .and_then(|_| f.read_to_end(&mut buffer))
                .map_err(|error| ContainerError::Read {
                    path: path.to_path_buf(),
                    error,
                })?;

            Container::from_bytes_and_version(buffer.into(), other).map_err(Into::into)
        }
        Err(e) => Err(ContainerError::Detect(e).into()),
    }
}

pub fn from_bytes(bytes: impl Into<Bytes>) -> Result<Container, WasmerPackageError> {
    let bytes: Bytes = bytes.into();

    if is_tarball(std::io::Cursor::new(&bytes)) {
        return parse_tarball(bytes.reader());
    }

    let version = webc::detect(bytes.as_ref())?;
    Container::from_bytes_and_version(bytes, version).map_err(Into::into)
}

#[allow(clippy::result_large_err)]
fn parse_tarball(reader: impl BufRead) -> Result<Container, WasmerPackageError> {
    let pkg = Package::from_tarball(reader)?;
    Ok(Container::new(pkg))
}

#[allow(clippy::result_large_err)]
fn parse_dir(path: &Path) -> Result<Container, WasmerPackageError> {
    let wasmer_toml = path.join("wasmer.toml");
    let pkg = Package::from_manifest(wasmer_toml)?;
    Ok(Container::new(pkg))
}

#[allow(clippy::result_large_err)]
fn parse_v1_mmap(f: File) -> Result<Container, ContainerError> {
    // We need to explicitly use WebcMmap to get a memory-mapped
    // parser
    let options = webc::v1::ParseOptions::default();
    let webc = webc::v1::WebCMmap::from_file(f, &options)?;
    Ok(Container::new(webc))
}

#[allow(clippy::result_large_err)]
fn parse_v2_mmap(f: File) -> Result<Container, ContainerError> {
    // Note: OwnedReader::from_file() will automatically try to
    // use a memory-mapped file when possible.
    let webc = webc::v2::read::OwnedReader::from_file(f)?;
    Ok(Container::new(webc))
}

#[allow(clippy::result_large_err)]
fn parse_v3_mmap(f: File) -> Result<Container, ContainerError> {
    // Note: OwnedReader::from_file() will automatically try to
    // use a memory-mapped file when possible.
    let webc = webc::v3::read::OwnedReader::from_file(f)?;
    Ok(Container::new(webc))
}

/// Convert a `Features` object to a list of WebAssembly feature strings
/// that can be used in annotations.
///
/// This maps each enabled feature to its corresponding string identifier
/// used in the WebAssembly ecosystem.
pub fn features_to_wasm_annotations(features: &Features) -> Vec<String> {
    let mut feature_strings = Vec::new();

    if features.simd {
        feature_strings.push("simd".to_string());
    }
    if features.bulk_memory {
        feature_strings.push("bulk-memory".to_string());
    }
    if features.reference_types {
        feature_strings.push("reference-types".to_string());
    }
    if features.multi_value {
        feature_strings.push("multi-value".to_string());
    }
    if features.threads {
        feature_strings.push("threads".to_string());
    }
    if features.exceptions {
        feature_strings.push("exception-handling".to_string());
    }
    if features.memory64 {
        feature_strings.push("memory64".to_string());
    }
    // Note: We don't currently include tail_call, module_linking, multi_memory,
    // relaxed_simd, or extended_const in the feature strings

    feature_strings
}

/// Create a `Features` object from a list of WebAssembly feature strings.
///
/// This is the inverse of `features_to_wasm_annotations`, mapping string identifiers
/// back to Features settings.
pub fn wasm_annotations_to_features(feature_strings: &[String]) -> Features {
    let mut features = Features::default();

    // Initialize with default values
    features
        .simd(false)
        .bulk_memory(false)
        .reference_types(false)
        .multi_value(false)
        .threads(false)
        .exceptions(false)
        .memory64(false);

    // Set features based on the string values
    for feature in feature_strings {
        match feature.as_str() {
            "simd" => {
                features.simd(true);
            }
            "bulk-memory" => {
                features.bulk_memory(true);
            }
            "reference-types" => {
                features.reference_types(true);
            }
            "multi-value" => {
                features.multi_value(true);
            }
            "threads" => {
                features.threads(true);
            }
            "exception-handling" => {
                features.exceptions(true);
            }
            "memory64" => {
                features.memory64(true);
            }
            // Ignore unrecognized features
            _ => {}
        }
    }

    features
}
