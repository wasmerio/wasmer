use std::{path::PathBuf, sync::Arc};

use bytes::Bytes;
use wasmer_vfs::{webc_fs::WebcFileSystem, FileSystem};
use webc::{
    metadata::Manifest,
    v1::{ParseOptions, WebC, WebCMmap, WebCOwned},
    Version,
};

/// A parsed WAPM package.
#[derive(Debug, Clone)]
pub struct WapmContainer {
    repr: Repr,
}

#[allow(dead_code)] // Some pub(crate) items are only used behind #[cfg] code
impl WapmContainer {
    /// Parses a .webc container file. Since .webc files
    /// can be very large, only file paths are allowed.
    pub fn from_path(path: PathBuf) -> std::result::Result<Self, WebcParseError> {
        let webc = webc::v1::WebCMmap::parse(path, &ParseOptions::default())?;
        Ok(Self {
            repr: Repr::V1Mmap(Arc::new(webc)),
        })
    }

    pub fn from_bytes(bytes: Bytes) -> std::result::Result<Self, WebcParseError> {
        match webc::detect(bytes.as_ref())? {
            Version::V1 => {
                let webc = WebCOwned::parse(bytes.into(), &ParseOptions::default())?;
                Ok(WapmContainer {
                    repr: Repr::V1Owned(Arc::new(webc)),
                })
            }
            Version::V2 => todo!(),
            other => Err(WebcParseError::UnsupportedVersion(other)),
        }
    }

    /// Returns the bytes of a file or a stringified error
    pub fn get_file<'b>(&'b self, path: &str) -> Result<&'b [u8], String> {
        match &self.repr {
            Repr::V1Mmap(mapped) => mapped
                .get_file(&mapped.get_package_name(), path)
                .map_err(|e| e.0),
            Repr::V1Owned(owned) => owned
                .get_file(&owned.get_package_name(), path)
                .map_err(|e| e.0),
        }
    }

    /// Returns a list of volumes in this container
    pub fn get_volumes(&self) -> Vec<String> {
        match &self.repr {
            Repr::V1Mmap(mapped) => mapped.volumes.keys().cloned().collect(),
            Repr::V1Owned(owned) => owned.volumes.keys().cloned().collect(),
        }
    }

    pub fn get_atom(&self, name: &str) -> Option<&[u8]> {
        match &self.repr {
            Repr::V1Mmap(mapped) => mapped.get_atom(&mapped.get_package_name(), name).ok(),
            Repr::V1Owned(owned) => owned.get_atom(&owned.get_package_name(), name).ok(),
        }
    }

    /// Lookup .wit bindings by name and parse them
    pub fn get_bindings<T: Bindings>(
        &self,
        bindings: &str,
    ) -> std::result::Result<T, ParseBindingsError> {
        let bindings = self
            .manifest()
            .bindings
            .iter()
            .find(|b| b.name == bindings)
            .ok_or_else(|| ParseBindingsError::NoBindings(bindings.to_string()))?;

        T::parse_bindings(self, &bindings.annotations).map_err(ParseBindingsError::ParseBindings)
    }

    pub fn manifest(&self) -> &Manifest {
        match &self.repr {
            Repr::V1Mmap(mapped) => &mapped.manifest,
            Repr::V1Owned(owned) => &owned.manifest,
        }
    }

    // HACK(Michael-F-Bryan): WapmContainer originally exposed its Arc<WebCMmap>
    // field, so every man and his dog accessed it directly instead of going
    // through the WapmContainer abstraction. This is an escape hatch to make
    // that code keep working for the time being.
    // #[deprecated]
    pub(crate) fn v1(&self) -> &WebC<'_> {
        match &self.repr {
            Repr::V1Mmap(mapped) => &*mapped,
            Repr::V1Owned(owned) => &*owned,
        }
    }

    /// Load a volume as a [`FileSystem`] node.
    pub(crate) fn volume_fs(&self, package_name: &str) -> Box<dyn FileSystem + Send + Sync> {
        match &self.repr {
            Repr::V1Mmap(mapped) => {
                Box::new(WebcFileSystem::init(Arc::clone(mapped), package_name))
            }
            Repr::V1Owned(owned) => Box::new(WebcFileSystem::init(Arc::clone(owned), package_name)),
        }
    }

    /// Get the entire container as a single filesystem.
    pub(crate) fn container_fs(&self) -> Box<dyn FileSystem + Send + Sync> {
        match &self.repr {
            Repr::V1Mmap(mapped) => Box::new(WebcFileSystem::init_all(Arc::clone(mapped))),
            Repr::V1Owned(owned) => Box::new(WebcFileSystem::init_all(Arc::clone(owned))),
        }
    }
}

#[derive(Debug, Clone)]
enum Repr {
    V1Mmap(Arc<WebCMmap>),
    V1Owned(Arc<WebCOwned>),
}

/// Error that happened while parsing .wit bindings
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd)]
pub enum ParseBindingsError {
    /// No bindings are available for the given lookup key
    NoBindings(String),
    /// Error happened during parsing
    ParseBindings(String),
}

/// Trait to parse bindings (any kind of bindings) for container .wasm files (usually .wit format)
pub trait Bindings {
    /// Function that takes annotations in a free-form `Value` struct and returns the parsed bindings or an error
    fn parse_bindings(_: &WapmContainer, _: &serde_cbor::Value) -> Result<Self, String>
    where
        Self: Sized;
}

/// WIT bindings
#[derive(Default, Debug, Copy, Clone)]
pub struct WitBindings {}

impl WitBindings {
    /// Unused: creates default wit bindings
    pub fn parse(_s: &str) -> Result<Self, String> {
        Ok(Self::default())
    }
}

impl Bindings for WitBindings {
    fn parse_bindings(
        container: &WapmContainer,
        value: &serde_cbor::Value,
    ) -> Result<Self, String> {
        let value: webc::metadata::BindingsExtended =
            serde_cbor::from_slice(&serde_cbor::to_vec(value).unwrap())
                .map_err(|e| format!("could not parse WitBindings annotations: {e}"))?;

        let mut wit_bindgen_filepath = value.exports().unwrap_or_default().to_string();

        for v in container.get_volumes() {
            let schema = format!("{v}://");
            if wit_bindgen_filepath.starts_with(&schema) {
                wit_bindgen_filepath = wit_bindgen_filepath.replacen(&schema, "", 1);
                break;
            }
        }

        let wit_bindings = container
            .get_file(&wit_bindgen_filepath)
            .map_err(|e| format!("could not get WitBindings file {wit_bindgen_filepath:?}: {e}"))?;

        let wit_bindings_str = std::str::from_utf8(wit_bindings)
            .map_err(|e| format!("could not get WitBindings file {wit_bindgen_filepath:?}: {e}"))?;

        Self::parse(wit_bindings_str)
    }
}

/// Error that ocurred while parsing the .webc file
#[derive(Debug)]
pub enum WebcParseError {
    /// Parse error
    Parse(webc::v1::Error),
    Detect(webc::DetectError),
    UnsupportedVersion(Version),
}

impl From<webc::v1::Error> for WebcParseError {
    fn from(e: webc::v1::Error) -> Self {
        WebcParseError::Parse(e)
    }
}

impl From<webc::DetectError> for WebcParseError {
    fn from(e: webc::DetectError) -> Self {
        WebcParseError::Detect(e)
    }
}
