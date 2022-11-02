#![cfg(feature = "webc_runner")]

use std::error::Error as StdError;
use std::path::PathBuf;
use std::sync::Arc;
use webc::*;

pub mod emscripten;
pub mod wasi;

/// Parsed WAPM file, memory-mapped to an on-disk path
#[derive(Debug, Clone)]
pub struct WapmContainer {
    /// WebC container
    pub webc: Arc<WebCMmap>,
}

impl core::ops::Deref for WapmContainer {
    type Target = webc::WebC<'static>;
    fn deref<'a>(&'a self) -> &WebC<'static> {
        &self.webc.webc
    }
}

/// Error that ocurred while parsing the .webc file
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum WebcParseError {
    /// Parse error
    Parse(webc::Error),
}

impl From<webc::Error> for WebcParseError {
    fn from(e: webc::Error) -> Self {
        WebcParseError::Parse(e)
    }
}

impl WapmContainer {
    /// Parses a .webc container file. Since .webc files
    /// can be very large, only file paths are allowed.
    pub fn new(path: PathBuf) -> std::result::Result<Self, WebcParseError> {
        let webc = webc::WebCMmap::parse(path, &webc::ParseOptions::default())?;
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
        let value: webc::WitBindingsExtended =
            serde_cbor::from_slice(&serde_cbor::to_vec(value).unwrap())
                .map_err(|e| format!("could not parse WitBindings annotations: {e}"))?;

        let mut wit_bindgen_filepath = value.wit.exports;

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

/// Trait that all runners have to implement
pub trait Runner {
    /// The return value of the output of the runner
    type Output;

    /// Returns whether the Runner will be able to run the `Command`
    fn can_run_command(
        &self,
        command_name: &str,
        command: &Command,
    ) -> Result<bool, Box<dyn StdError>>;

    /// Implementation to run the given command
    ///
    /// - use `cmd.annotations` to get the metadata for the given command
    /// - use `container.get_atom()` to get the
    fn run_command(
        &mut self,
        command_name: &str,
        cmd: &Command,
        container: &WapmContainer,
    ) -> Result<Self::Output, Box<dyn StdError>>;

    /// Runs the container if the container has an `entrypoint` in the manifest
    fn run(&mut self, container: &WapmContainer) -> Result<Self::Output, Box<dyn StdError>> {
        let cmd = match container.webc.webc.manifest.entrypoint.as_ref() {
            Some(s) => s,
            None => {
                let path = format!("{}", container.webc.path.display());
                return Err(Box::new(webc::Error(format!(
                    "Cannot run {path:?}: not executable (no entrypoint in manifest)"
                ))));
            }
        };

        self.run_cmd(container, cmd)
    }

    /// Runs the given `cmd` on the container
    fn run_cmd(
        &mut self,
        container: &WapmContainer,
        cmd: &str,
    ) -> Result<Self::Output, Box<dyn StdError>> {
        let path = format!("{}", container.webc.path.display());
        let command_to_exec = container
            .webc
            .webc
            .manifest
            .commands
            .get(cmd)
            .ok_or_else(|| anyhow::anyhow!("{path}: command {cmd:?} not found in manifest"))?;

        let _path = format!("{}", container.webc.path.display());

        match self.can_run_command(cmd, command_to_exec) {
            Ok(true) => {}
            Ok(false) => {
                return Err(Box::new(webc::Error(format!(
                    "Cannot run command {cmd:?} with runner {:?}",
                    command_to_exec.runner
                ))));
            }
            Err(e) => {
                return Err(Box::new(webc::Error(format!(
                    "Cannot run command {cmd:?} with runner {:?}: {e}",
                    command_to_exec.runner
                ))));
            }
        }

        self.run_command(cmd, command_to_exec, container)
    }
}
