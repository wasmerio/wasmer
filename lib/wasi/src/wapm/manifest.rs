use std::{collections::HashMap, fmt, path::PathBuf};

use semver::Version;
use serde::*;

/// The name of the manifest file. This is hard-coded for now.
pub static MANIFEST_FILE_NAME: &str = "wapm.toml";
pub static PACKAGES_DIR_NAME: &str = "wapm_packages";

/// Primitive wasm type
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WasmType {
    I32,
    I64,
    F32,
    F64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Import {
    Func {
        namespace: String,
        name: String,
        params: Vec<WasmType>,
        result: Vec<WasmType>,
    },
    Global {
        namespace: String,
        name: String,
        var_type: WasmType,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Export {
    Func {
        name: String,
        params: Vec<WasmType>,
        result: Vec<WasmType>,
    },
    Global {
        name: String,
        var_type: WasmType,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Interface {
    /// The name the interface gave itself
    pub name: Option<String>,
    /// Things that the module can import
    pub imports: HashMap<(String, String), Import>,
    /// Things that the module must export
    pub exports: HashMap<String, Export>,
}

/// The ABI is a hint to WebAssembly runtimes about what additional imports to insert.
/// It currently is only used for validation (in the validation subcommand).  The default value is `None`.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
pub enum Abi {
    #[serde(rename = "emscripten")]
    Emscripten,
    #[serde(rename = "none")]
    None,
    #[serde(rename = "wasi")]
    Wasi,
}

impl Abi {
    pub fn to_str(&self) -> &str {
        match self {
            Abi::Emscripten => "emscripten",
            Abi::Wasi => "wasi",
            Abi::None => "generic",
        }
    }
    pub fn is_none(&self) -> bool {
        return self == &Abi::None;
    }
    pub fn from_str(name: &str) -> Self {
        match name.to_lowercase().as_ref() {
            "emscripten" => Abi::Emscripten,
            "wasi" => Abi::Wasi,
            _ => Abi::None,
        }
    }
}

impl fmt::Display for Abi {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.to_str())
    }
}

impl Default for Abi {
    fn default() -> Self {
        Abi::None
    }
}

impl Abi {
    pub fn get_interface(&self) -> Option<Interface> {
        match self {
            Abi::Emscripten => None,
            Abi::Wasi => None,
            Abi::None => None,
        }
    }
}

/// Describes a command for a wapm module
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Package {
    pub name: String,
    pub version: Version,
    pub description: String,
    pub license: Option<String>,
    /// The location of the license file, useful for non-standard licenses
    #[serde(rename = "license-file")]
    pub license_file: Option<PathBuf>,
    pub readme: Option<PathBuf>,
    pub repository: Option<String>,
    pub homepage: Option<String>,
    #[serde(rename = "wasmer-extra-flags")]
    pub wasmer_extra_flags: Option<String>,
    #[serde(
        rename = "disable-command-rename",
        default,
        skip_serializing_if = "std::ops::Not::not"
    )]
    pub disable_command_rename: bool,
    /// Unlike, `disable-command-rename` which prevents `wapm run <Module name>`,
    /// this flag enables the command rename of `wapm run <COMMAND_NAME>` into
    /// just `<COMMAND_NAME>. This is useful for programs that need to inspect
    /// their argv[0] names and when the command name matches their executable name.
    #[serde(
        rename = "rename-commands-to-raw-command-name",
        default,
        skip_serializing_if = "std::ops::Not::not"
    )]
    pub rename_commands_to_raw_command_name: bool,
}

/// Describes a command for a wapm module
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Command {
    pub name: String,
    pub module: String,
    pub main_args: Option<String>,
    pub package: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Module {
    pub name: String,
    pub source: PathBuf,
    #[serde(default = "Abi::default", skip_serializing_if = "Abi::is_none")]
    pub abi: Abi,
    #[cfg(feature = "package")]
    pub fs: Option<Table>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interfaces: Option<HashMap<String, String>>,
}

/// The manifest represents the file used to describe a Wasm package.
///
/// The `module` field represents the wasm file to be published.
///
/// The `source` is used to create bundles with the `fs` section.
///
/// The `fs` section represents fs assets that will be made available to the
/// program relative to its starting current directory (there may be issues with WASI).
/// These are pairs of paths.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Manifest {
    pub package: Package,
    pub dependencies: Option<HashMap<String, String>>,
    pub module: Option<Vec<Module>>,
    pub command: Option<Vec<Command>>,
    /// Of the form Guest -> Host path
    pub fs: Option<HashMap<String, PathBuf>>,
    /// private data
    /// store the directory path of the manifest file for use later accessing relative path fields
    #[serde(skip)]
    pub base_directory_path: PathBuf,
}
