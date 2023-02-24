use std::{path::PathBuf, sync::Arc};

use webc::{metadata::Manifest, v1::WebCMmap};

/// Parsed WAPM file, memory-mapped to an on-disk path
#[derive(Debug, Clone)]
pub struct WapmContainer {
    /// WebC container
    webc: Arc<WebCMmap>,
}

impl WapmContainer {
    /// Parses a .webc container file. Since .webc files
    /// can be very large, only file paths are allowed.
    pub fn new(path: PathBuf) -> std::result::Result<Self, WebcParseError> {
        let webc = webc::v1::WebCMmap::parse(path, &webc::v1::ParseOptions::default())?;
        Ok(Self {
            webc: Arc::new(webc),
        })
    }

    /// Returns the bytes of a file or a stringified error
    pub fn get_file<'b>(&'b self, path: &str) -> Result<&'b [u8], String> {
        self.webc
            .get_file(&self.webc.get_package_name(), path)
            .map_err(|e| e.0)
    }

    /// Returns a list of volumes in this container
    pub fn get_volumes(&self) -> Vec<String> {
        self.webc.volumes.keys().cloned().collect::<Vec<_>>()
    }

    /// Lookup .wit bindings by name and parse them
    pub fn get_bindings<T: Bindings>(
        &self,
        bindings: &str,
    ) -> std::result::Result<T, ParseBindingsError> {
        let bindings = self
            .webc
            .manifest
            .bindings
            .iter()
            .find(|b| b.name == bindings)
            .ok_or_else(|| ParseBindingsError::NoBindings(bindings.to_string()))?;

        T::parse_bindings(self, &bindings.annotations).map_err(ParseBindingsError::ParseBindings)
    }

    pub fn manifest(&self) -> &Manifest {
        &self.webc.manifest
    }

    pub fn v1(&self) -> &Arc<WebCMmap> {
        &self.webc
    }
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
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum WebcParseError {
    /// Parse error
    Parse(webc::v1::Error),
}

impl From<webc::v1::Error> for WebcParseError {
    fn from(e: webc::v1::Error) -> Self {
        WebcParseError::Parse(e)
    }
}
