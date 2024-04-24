//! Wasmer package definitions.
//!
//! Describes the contents of a `wasmer.toml` file.

#![allow(deprecated)]

mod error;
mod named_package_ident;
mod package_hash;
mod package_id;
mod package_ident;
mod package_source;

pub use self::{
    error::PackageParseError,
    named_package_ident::{NamedPackageIdent, Tag},
    package_hash::PackageHash,
    package_id::{NamedPackageId, PackageId},
    package_ident::PackageIdent,
    package_source::PackageSource,
};

use std::{
    borrow::Cow,
    collections::{hash_map::HashMap, BTreeMap, BTreeSet},
    fmt::{self, Display},
    path::{Path, PathBuf},
    str::FromStr,
};

use indexmap::IndexMap;
use semver::{Version, VersionReq};
use serde::{de::Error as _, Deserialize, Serialize};
use thiserror::Error;

/// The ABI is a hint to WebAssembly runtimes about what additional imports to
/// insert and how a module may be run.
///
/// If not specified, [`Abi::None`] is the default.
#[derive(Clone, Copy, Default, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[non_exhaustive]
pub enum Abi {
    #[serde(rename = "emscripten")]
    Emscripten,
    #[default]
    #[serde(rename = "none")]
    None,
    #[serde(rename = "wasi")]
    Wasi,
    #[serde(rename = "wasm4")]
    WASM4,
}

impl Abi {
    /// Get the ABI's human-friendly name.
    pub fn to_str(&self) -> &str {
        match self {
            Abi::Emscripten => "emscripten",
            Abi::Wasi => "wasi",
            Abi::WASM4 => "wasm4",
            Abi::None => "generic",
        }
    }

    /// Is this a [`Abi::None`]?
    pub fn is_none(&self) -> bool {
        matches!(self, Abi::None)
    }

    /// Create an [`Abi`] from its human-friendly name.
    pub fn from_name(name: &str) -> Self {
        name.parse().unwrap_or(Abi::None)
    }
}

impl fmt::Display for Abi {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.to_str())
    }
}

impl FromStr for Abi {
    type Err = Box<dyn std::error::Error + Send + Sync>;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "emscripten" => Ok(Abi::Emscripten),
            "wasi" => Ok(Abi::Wasi),
            "wasm4" => Ok(Abi::WASM4),
            "generic" => Ok(Abi::None),
            _ => Err(format!("Unknown ABI, \"{s}\"").into()),
        }
    }
}

/// The default name for the manifest file.
pub static MANIFEST_FILE_NAME: &str = "wasmer.toml";

const README_PATHS: &[&str; 5] = &[
    "README",
    "README.md",
    "README.markdown",
    "README.mdown",
    "README.mkdn",
];

const LICENSE_PATHS: &[&str; 3] = &["LICENSE", "LICENSE.md", "COPYING"];

/// Package definition for a Wasmer package.
///
/// Usually stored in a `wasmer.toml` file.
#[derive(Clone, Debug, Deserialize, Serialize, derive_builder::Builder)]
#[non_exhaustive]
pub struct Package {
    /// The package's name in the form `namespace/name`.
    #[builder(setter(into))]
    pub name: String,
    /// The package's version number.
    pub version: Version,
    /// A brief description of the package.
    #[builder(setter(into))]
    pub description: String,
    /// A SPDX license specifier for this package.
    #[builder(setter(into, strip_option), default)]
    pub license: Option<String>,
    /// The location of the license file, useful for non-standard licenses
    #[serde(rename = "license-file")]
    #[builder(setter(into, strip_option), default)]
    pub license_file: Option<PathBuf>,
    /// The package's README file.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(into, strip_option), default)]
    pub readme: Option<PathBuf>,
    /// A URL pointing to the package's source code.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(into, strip_option), default)]
    pub repository: Option<String>,
    /// The website used as the package's homepage.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(into, strip_option), default)]
    pub homepage: Option<String>,
    #[serde(rename = "wasmer-extra-flags")]
    #[builder(setter(into, strip_option), default)]
    #[deprecated(
        since = "0.9.2",
        note = "Use runner-specific command attributes instead"
    )]
    pub wasmer_extra_flags: Option<String>,
    #[serde(
        rename = "disable-command-rename",
        default,
        skip_serializing_if = "std::ops::Not::not"
    )]
    #[builder(default)]
    #[deprecated(
        since = "0.9.2",
        note = "Does nothing. Prefer a runner-specific command attribute instead"
    )]
    pub disable_command_rename: bool,
    /// Unlike, `disable-command-rename` which prevents `wasmer run <Module name>`,
    /// this flag enables the command rename of `wasmer run <COMMAND_NAME>` into
    /// just `<COMMAND_NAME>`. This is useful for programs that need to inspect
    /// their `argv[0]` names and when the command name matches their executable
    /// name.
    #[serde(
        rename = "rename-commands-to-raw-command-name",
        default,
        skip_serializing_if = "std::ops::Not::not"
    )]
    #[builder(default)]
    #[deprecated(
        since = "0.9.2",
        note = "Does nothing. Prefer a runner-specific command attribute instead"
    )]
    pub rename_commands_to_raw_command_name: bool,
    /// The name of the command that should be used by `wasmer run` by default.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(into, strip_option), default)]
    pub entrypoint: Option<String>,
    /// Mark this as a private package
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[builder(default)]
    pub private: bool,
}

impl Package {
    /// Create a [`PackageBuilder`] populated with all mandatory fields.
    pub fn builder(
        name: impl Into<String>,
        version: Version,
        description: impl Into<String>,
    ) -> PackageBuilder {
        PackageBuilder::new(name, version, description)
    }
}

impl PackageBuilder {
    pub fn new(name: impl Into<String>, version: Version, description: impl Into<String>) -> Self {
        let mut builder = PackageBuilder::default();
        builder.name(name).version(version).description(description);
        builder
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Command {
    V1(CommandV1),
    V2(CommandV2),
}

impl Command {
    /// Get the command's name.
    pub fn get_name(&self) -> &str {
        match self {
            Self::V1(c) => &c.name,
            Self::V2(c) => &c.name,
        }
    }

    /// Get the module this [`Command`] refers to.
    pub fn get_module(&self) -> &ModuleReference {
        match self {
            Self::V1(c) => &c.module,
            Self::V2(c) => &c.module,
        }
    }
}

/// Describes a command for a wasmer module.
///
/// When a command is deserialized using [`CommandV1`], the runner is inferred
/// by looking at the [`Abi`] from the [`Module`] it refers to.
///
/// If possible, prefer to use the [`CommandV2`] format.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)] // Note: needed to prevent accidentally parsing
// a CommandV2 as a CommandV1
#[deprecated(since = "0.9.2", note = "Prefer the CommandV2 syntax")]
pub struct CommandV1 {
    pub name: String,
    pub module: ModuleReference,
    pub main_args: Option<String>,
    pub package: Option<String>,
}

/// An executable command.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct CommandV2 {
    /// The name of the command.
    pub name: String,
    /// The module containing this command's executable.
    pub module: ModuleReference,
    /// The runner to use when running this command.
    ///
    /// This may be a URL, or the well-known runners `wasi`, `wcgi`, or
    /// `emscripten`.
    pub runner: String,
    /// Extra annotations that will be consumed by the runner.
    pub annotations: Option<CommandAnnotations>,
}

impl CommandV2 {
    /// Get annotations, automatically loading them from a file relative to the
    /// `wasmer.toml`'s directory, if necessary.
    pub fn get_annotations(&self, basepath: &Path) -> Result<Option<serde_cbor::Value>, String> {
        match self.annotations.as_ref() {
            Some(CommandAnnotations::Raw(v)) => Ok(Some(toml_to_cbor_value(v))),
            Some(CommandAnnotations::File(FileCommandAnnotations { file, kind })) => {
                let path = basepath.join(file.clone());
                let file = std::fs::read_to_string(&path).map_err(|e| {
                    format!(
                        "Error reading {:?}.annotation ({:?}): {e}",
                        self.name,
                        path.display()
                    )
                })?;
                match kind {
                    FileKind::Json => {
                        let value: serde_json::Value =
                            serde_json::from_str(&file).map_err(|e| {
                                format!(
                                    "Error reading {:?}.annotation ({:?}): {e}",
                                    self.name,
                                    path.display()
                                )
                            })?;
                        Ok(Some(json_to_cbor_value(&value)))
                    }
                    FileKind::Yaml => {
                        let value: serde_yaml::Value =
                            serde_yaml::from_str(&file).map_err(|e| {
                                format!(
                                    "Error reading {:?}.annotation ({:?}): {e}",
                                    self.name,
                                    path.display()
                                )
                            })?;
                        Ok(Some(yaml_to_cbor_value(&value)))
                    }
                }
            }
            None => Ok(None),
        }
    }
}

/// A reference to a module which may or may not come from another package.
///
/// # Serialization
///
/// A [`ModuleReference`] is serialized via its [`String`] representation.
#[derive(Clone, Debug, PartialEq)]
pub enum ModuleReference {
    /// A module in the current package.
    CurrentPackage {
        /// The name of the module.
        module: String,
    },
    /// A module that will be provided by a dependency, in `dependency:module`
    /// form.
    Dependency {
        /// The name of the dependency the module comes from.
        dependency: String,
        /// The name of the module.
        module: String,
    },
}

impl Serialize for ModuleReference {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_string().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ModuleReference {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let repr: Cow<'de, str> = Cow::deserialize(deserializer)?;
        repr.parse().map_err(D::Error::custom)
    }
}

impl FromStr for ModuleReference {
    type Err = Box<dyn std::error::Error + Send + Sync>;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.split_once(':') {
            Some((dependency, module)) => {
                if module.contains(':') {
                    return Err("Invalid format".into());
                }

                Ok(ModuleReference::Dependency {
                    dependency: dependency.to_string(),
                    module: module.to_string(),
                })
            }
            None => Ok(ModuleReference::CurrentPackage {
                module: s.to_string(),
            }),
        }
    }
}

impl Display for ModuleReference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModuleReference::CurrentPackage { module } => Display::fmt(module, f),
            ModuleReference::Dependency { dependency, module } => {
                write!(f, "{dependency}:{module}")
            }
        }
    }
}

fn toml_to_cbor_value(val: &toml::Value) -> serde_cbor::Value {
    match val {
        toml::Value::String(s) => serde_cbor::Value::Text(s.clone()),
        toml::Value::Integer(i) => serde_cbor::Value::Integer(*i as i128),
        toml::Value::Float(f) => serde_cbor::Value::Float(*f),
        toml::Value::Boolean(b) => serde_cbor::Value::Bool(*b),
        toml::Value::Datetime(d) => serde_cbor::Value::Text(format!("{}", d)),
        toml::Value::Array(sq) => {
            serde_cbor::Value::Array(sq.iter().map(toml_to_cbor_value).collect())
        }
        toml::Value::Table(m) => serde_cbor::Value::Map(
            m.iter()
                .map(|(k, v)| (serde_cbor::Value::Text(k.clone()), toml_to_cbor_value(v)))
                .collect(),
        ),
    }
}

fn json_to_cbor_value(val: &serde_json::Value) -> serde_cbor::Value {
    match val {
        serde_json::Value::Null => serde_cbor::Value::Null,
        serde_json::Value::Bool(b) => serde_cbor::Value::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                serde_cbor::Value::Integer(i as i128)
            } else if let Some(u) = n.as_u64() {
                serde_cbor::Value::Integer(u as i128)
            } else if let Some(f) = n.as_f64() {
                serde_cbor::Value::Float(f)
            } else {
                serde_cbor::Value::Null
            }
        }
        serde_json::Value::String(s) => serde_cbor::Value::Text(s.clone()),
        serde_json::Value::Array(sq) => {
            serde_cbor::Value::Array(sq.iter().map(json_to_cbor_value).collect())
        }
        serde_json::Value::Object(m) => serde_cbor::Value::Map(
            m.iter()
                .map(|(k, v)| (serde_cbor::Value::Text(k.clone()), json_to_cbor_value(v)))
                .collect(),
        ),
    }
}

fn yaml_to_cbor_value(val: &serde_yaml::Value) -> serde_cbor::Value {
    match val {
        serde_yaml::Value::Null => serde_cbor::Value::Null,
        serde_yaml::Value::Bool(b) => serde_cbor::Value::Bool(*b),
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                serde_cbor::Value::Integer(i as i128)
            } else if let Some(u) = n.as_u64() {
                serde_cbor::Value::Integer(u as i128)
            } else if let Some(f) = n.as_f64() {
                serde_cbor::Value::Float(f)
            } else {
                serde_cbor::Value::Null
            }
        }
        serde_yaml::Value::String(s) => serde_cbor::Value::Text(s.clone()),
        serde_yaml::Value::Sequence(sq) => {
            serde_cbor::Value::Array(sq.iter().map(yaml_to_cbor_value).collect())
        }
        serde_yaml::Value::Mapping(m) => serde_cbor::Value::Map(
            m.iter()
                .map(|(k, v)| (yaml_to_cbor_value(k), yaml_to_cbor_value(v)))
                .collect(),
        ),
        serde_yaml::Value::Tagged(tag) => yaml_to_cbor_value(&tag.value),
    }
}

/// Annotations for a command.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
#[repr(C)]
pub enum CommandAnnotations {
    /// Annotations that will be read from a file on disk.
    File(FileCommandAnnotations),
    /// Annotations that are specified inline.
    Raw(toml::Value),
}

/// Annotations on disk.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct FileCommandAnnotations {
    /// The path to the annotations file.
    pub file: PathBuf,
    /// Which format are the annotations saved in?
    pub kind: FileKind,
}

/// The different formats that [`FileCommandAnnotations`] can be saved in.
#[derive(Debug, Copy, Clone, PartialEq, PartialOrd, Ord, Eq, Deserialize, Serialize)]
pub enum FileKind {
    /// A `*.yaml` file that will be deserialized using [`serde_yaml`].
    #[serde(rename = "yaml")]
    Yaml,
    /// A `*.json` file that will be deserialized using [`serde_json`].
    #[serde(rename = "json")]
    Json,
}

/// A file which may be executed by a [`Command`]. Sometimes also referred to as
/// an "atom".
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Module {
    /// The name used to refer to this module.
    pub name: String,
    /// The location of the module file on disk, relative to the manifest
    /// directory.
    pub source: PathBuf,
    /// The ABI this module satisfies.
    #[serde(default = "Abi::default", skip_serializing_if = "Abi::is_none")]
    pub abi: Abi,
    #[serde(default)]
    pub kind: Option<String>,
    /// WebAssembly interfaces this module requires.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interfaces: Option<HashMap<String, String>>,
    /// Interface definitions that can be used to generate bindings to this
    /// module.
    pub bindings: Option<Bindings>,
}

/// The interface exposed by a [`Module`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Bindings {
    Wit(WitBindings),
    Wai(WaiBindings),
}

impl Bindings {
    /// Get all files that make up this interface.
    ///
    /// For all binding types except [`WitBindings`], this will recursively
    /// look for any files that are imported.
    ///
    /// The caller can assume that any path that was referenced exists.
    pub fn referenced_files(&self, base_directory: &Path) -> Result<Vec<PathBuf>, ImportsError> {
        match self {
            Bindings::Wit(WitBindings { wit_exports, .. }) => {
                // Note: we explicitly don't support imported files with WIT
                // because wit-bindgen's wit-parser crate isn't on crates.io.

                let path = base_directory.join(wit_exports);

                if path.exists() {
                    Ok(vec![path])
                } else {
                    Err(ImportsError::FileNotFound(path))
                }
            }
            Bindings::Wai(wai) => wai.referenced_files(base_directory),
        }
    }
}

impl Serialize for Bindings {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Bindings::Wit(w) => w.serialize(serializer),
            Bindings::Wai(w) => w.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for Bindings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = toml::Value::deserialize(deserializer)?;

        let keys = ["wit-bindgen", "wai-version"];
        let [wit_bindgen, wai_version] = keys.map(|key| value.get(key).is_some());

        match (wit_bindgen, wai_version) {
            (true, false) => WitBindings::deserialize(value)
                .map(Bindings::Wit)
                .map_err(D::Error::custom),
            (false, true) => WaiBindings::deserialize(value)
                .map(Bindings::Wai)
                .map_err(D::Error::custom),
            (true, true) | (false, false) => {
                let msg = format!(
                    "expected one of \"{}\" to be provided, but not both",
                    keys.join("\" or \""),
                );
                Err(D::Error::custom(msg))
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct WitBindings {
    /// The version of the WIT format being used.
    pub wit_bindgen: Version,
    /// The `*.wit` file's location on disk.
    pub wit_exports: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct WaiBindings {
    /// The version of the WAI format being used.
    pub wai_version: Version,
    /// The `*.wai` file defining the interface this package exposes.
    pub exports: Option<PathBuf>,
    /// The `*.wai` files for any functionality this package imports from the
    /// host.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub imports: Vec<PathBuf>,
}

impl WaiBindings {
    fn referenced_files(&self, base_directory: &Path) -> Result<Vec<PathBuf>, ImportsError> {
        let WaiBindings {
            exports, imports, ..
        } = self;

        // Note: WAI files may import other WAI files, so we start with all
        // WAI files mentioned in the wasmer.toml then recursively add their
        // imports.

        let initial_paths = exports
            .iter()
            .chain(imports)
            .map(|relative_path| base_directory.join(relative_path));

        let mut to_check: Vec<PathBuf> = Vec::new();

        for path in initial_paths {
            if !path.exists() {
                return Err(ImportsError::FileNotFound(path));
            }
            to_check.push(path);
        }

        let mut files = BTreeSet::new();

        while let Some(path) = to_check.pop() {
            if files.contains(&path) {
                continue;
            }

            to_check.extend(get_imported_wai_files(&path)?);
            files.insert(path);
        }

        Ok(files.into_iter().collect())
    }
}

/// Parse a `*.wai` file to find the absolute path for any other `*.wai` files
/// it may import, relative to the original `*.wai` file.
///
/// This function makes sure any imported files exist.
fn get_imported_wai_files(path: &Path) -> Result<Vec<PathBuf>, ImportsError> {
    let _wai_src = std::fs::read_to_string(path).map_err(|error| ImportsError::Read {
        path: path.to_path_buf(),
        error,
    })?;

    let parent_dir = path.parent()
            .expect("All paths should have a parent directory because we joined them relative to the base directory");

    // TODO(Michael-F-Bryan): update the wai-parser crate to give you access to
    // the imported interfaces. For now, we just pretend there are no import
    // statements in the *.wai file.
    let raw_imports: Vec<String> = Vec::new();

    // Note: imported paths in a *.wai file are all relative, so we need to
    // resolve their absolute path relative to the original *.wai file.
    let mut resolved_paths = Vec::new();

    for imported in raw_imports {
        let absolute_path = parent_dir.join(imported);

        if !absolute_path.exists() {
            return Err(ImportsError::ImportedFileNotFound {
                path: absolute_path,
                referenced_by: path.to_path_buf(),
            });
        }

        resolved_paths.push(absolute_path);
    }

    Ok(resolved_paths)
}

/// Errors that may occur when resolving [`Bindings`] imports.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ImportsError {
    #[error(
        "The \"{}\" mentioned in the manifest doesn't exist",
        _0.display(),
    )]
    FileNotFound(PathBuf),
    #[error(
        "The \"{}\" imported by \"{}\" doesn't exist",
        path.display(),
        referenced_by.display(),
    )]
    ImportedFileNotFound {
        path: PathBuf,
        referenced_by: PathBuf,
    },
    #[error("Unable to parse \"{}\" as a WAI file", path.display())]
    WaiParse { path: PathBuf },
    #[error("Unable to read \"{}\"", path.display())]
    Read {
        path: PathBuf,
        #[source]
        error: std::io::Error,
    },
}

/// The manifest represents the file used to describe a Wasm package.
#[derive(Clone, Debug, Deserialize, Serialize, derive_builder::Builder)]
#[non_exhaustive]
pub struct Manifest {
    /// Metadata about the package itself.
    pub package: Option<Package>,
    /// The package's dependencies.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    #[builder(default)]
    pub dependencies: HashMap<String, VersionReq>,
    /// The mappings used when making bundled assets available to WebAssembly
    /// instances, in the form guest -> host.
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    #[builder(default)]
    pub fs: IndexMap<String, PathBuf>,
    /// WebAssembly modules to be published.
    #[serde(default, rename = "module", skip_serializing_if = "Vec::is_empty")]
    #[builder(default)]
    pub modules: Vec<Module>,
    /// Commands the package makes available to users.
    #[serde(default, rename = "command", skip_serializing_if = "Vec::is_empty")]
    #[builder(default)]
    pub commands: Vec<Command>,
}

impl Manifest {
    /// Create a [`ManifestBuilder`] populated with all mandatory fields.
    pub fn builder(package: Package) -> ManifestBuilder {
        ManifestBuilder::new(package)
    }

    /// Parse a [`Manifest`] from its TOML representation.
    pub fn parse(s: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(s)
    }

    /// Construct a manifest by searching in the specified directory for a
    /// manifest file.
    pub fn find_in_directory<T: AsRef<Path>>(path: T) -> Result<Self, ManifestError> {
        let path = path.as_ref();

        if !path.is_dir() {
            return Err(ManifestError::MissingManifest(path.to_path_buf()));
        }
        let manifest_path_buf = path.join(MANIFEST_FILE_NAME);
        let contents = std::fs::read_to_string(&manifest_path_buf)
            .map_err(|_e| ManifestError::MissingManifest(manifest_path_buf))?;
        let mut manifest: Self = toml::from_str(contents.as_str())?;

        if let Some(package) = manifest.package.as_mut() {
            if package.readme.is_none() {
                package.readme = locate_file(path, README_PATHS);
            }

            if package.license_file.is_none() {
                package.license_file = locate_file(path, LICENSE_PATHS);
            }
        }
        manifest.validate()?;

        Ok(manifest)
    }

    /// Validate this [`Manifest`] to check for common semantic errors.
    ///
    /// Some common error cases are:
    ///
    /// - Having multiple modules with the same name
    /// - Having multiple commands with the same name
    /// - A [`Command`] that references a non-existent [`Module`] in the current
    ///   package
    /// - A [`Package::entrypoint`] which points to a non-existent [`Command`]
    pub fn validate(&self) -> Result<(), ValidationError> {
        let mut modules = BTreeMap::new();

        for module in &self.modules {
            let is_duplicate = modules.insert(&module.name, module).is_some();

            if is_duplicate {
                return Err(ValidationError::DuplicateModule {
                    name: module.name.clone(),
                });
            }
        }

        let mut commands = BTreeMap::new();

        for command in &self.commands {
            let is_duplicate = commands.insert(command.get_name(), command).is_some();

            if is_duplicate {
                return Err(ValidationError::DuplicateCommand {
                    name: command.get_name().to_string(),
                });
            }

            let module_reference = command.get_module();
            match &module_reference {
                ModuleReference::CurrentPackage { module } => {
                    if let Some(module) = modules.get(&module) {
                        if module.abi == Abi::None && module.interfaces.is_none() {
                            return Err(ValidationError::MissingABI {
                                command: command.get_name().to_string(),
                                module: module.name.clone(),
                            });
                        }
                    } else {
                        return Err(ValidationError::MissingModuleForCommand {
                            command: command.get_name().to_string(),
                            module: command.get_module().clone(),
                        });
                    }
                }
                ModuleReference::Dependency { dependency, .. } => {
                    // We don't have access to the dependency so just assume
                    // the module is correct.
                    if !self.dependencies.contains_key(dependency) {
                        return Err(ValidationError::MissingDependency {
                            command: command.get_name().to_string(),
                            dependency: dependency.clone(),
                            module_ref: module_reference.clone(),
                        });
                    }
                }
            }
        }

        if let Some(package) = &self.package {
            if let Some(entrypoint) = package.entrypoint.as_deref() {
                if !commands.contains_key(entrypoint) {
                    return Err(ValidationError::InvalidEntrypoint {
                        entrypoint: entrypoint.to_string(),
                        available_commands: commands.keys().map(ToString::to_string).collect(),
                    });
                }
            }
        }

        Ok(())
    }

    /// add a dependency
    pub fn add_dependency(&mut self, dependency_name: String, dependency_version: VersionReq) {
        self.dependencies
            .insert(dependency_name, dependency_version);
    }

    /// remove dependency by package name
    pub fn remove_dependency(&mut self, dependency_name: &str) -> Option<VersionReq> {
        self.dependencies.remove(dependency_name)
    }

    /// Convert a [`Manifest`] to its TOML representation.
    pub fn to_string(&self) -> anyhow::Result<String> {
        let repr = toml::to_string_pretty(&self)?;
        Ok(repr)
    }

    /// Write the manifest to permanent storage
    pub fn save(&self, path: impl AsRef<Path>) -> anyhow::Result<()> {
        let manifest = toml::to_string_pretty(self)?;
        std::fs::write(path, manifest).map_err(ManifestError::CannotSaveManifest)?;
        Ok(())
    }
}

fn locate_file(path: &Path, candidates: &[&str]) -> Option<PathBuf> {
    for filename in candidates {
        let path_buf = path.join(filename);
        if path_buf.exists() {
            return Some(filename.into());
        }
    }
    None
}

impl ManifestBuilder {
    pub fn new(package: Package) -> Self {
        let mut builder = ManifestBuilder::default();
        builder.package(Some(package));
        builder
    }

    /// Include a directory on the host in the package and make it available to
    /// a WebAssembly guest at the `guest` path.
    pub fn map_fs(&mut self, guest: impl Into<String>, host: impl Into<PathBuf>) -> &mut Self {
        self.fs
            .get_or_insert_with(IndexMap::new)
            .insert(guest.into(), host.into());
        self
    }

    /// Add a dependency to the [`Manifest`].
    pub fn with_dependency(&mut self, name: impl Into<String>, version: VersionReq) -> &mut Self {
        self.dependencies
            .get_or_insert_with(HashMap::new)
            .insert(name.into(), version);
        self
    }

    /// Add a [`Module`] to the [`Manifest`].
    pub fn with_module(&mut self, module: Module) -> &mut Self {
        self.modules.get_or_insert_with(Vec::new).push(module);
        self
    }

    /// Add a [`Command`] to the [`Manifest`].
    pub fn with_command(&mut self, command: Command) -> &mut Self {
        self.commands.get_or_insert_with(Vec::new).push(command);
        self
    }
}

/// Errors that may occur while working with a [`Manifest`].
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ManifestError {
    #[error("Manifest file not found at \"{}\"", _0.display())]
    MissingManifest(PathBuf),
    #[error("Could not save manifest file: {0}.")]
    CannotSaveManifest(#[source] std::io::Error),
    #[error("Could not parse manifest because {0}.")]
    TomlParseError(#[from] toml::de::Error),
    #[error("There was an error validating the manifest")]
    ValidationError(#[from] ValidationError),
}

/// Errors that may be returned by [`Manifest::validate()`].
#[derive(Debug, PartialEq, Error)]
#[non_exhaustive]
pub enum ValidationError {
    #[error(
        "missing ABI field on module, \"{module}\", used by command, \"{command}\"; an ABI of `wasi` or `emscripten` is required",
    )]
    MissingABI { command: String, module: String },
    #[error("missing module, \"{module}\", in manifest used by command, \"{command}\"")]
    MissingModuleForCommand {
        command: String,
        module: ModuleReference,
    },
    #[error("The \"{command}\" command refers to a nonexistent dependency, \"{dependency}\" in \"{module_ref}\"")]
    MissingDependency {
        command: String,
        dependency: String,
        module_ref: ModuleReference,
    },
    #[error("The entrypoint, \"{entrypoint}\", isn't a valid command (commands: {})", available_commands.join(", "))]
    InvalidEntrypoint {
        entrypoint: String,
        available_commands: Vec<String>,
    },
    #[error("Duplicate module, \"{name}\"")]
    DuplicateModule { name: String },
    #[error("Duplicate command, \"{name}\"")]
    DuplicateCommand { name: String },
}

#[cfg(test)]
mod tests {
    use std::fmt::Debug;

    use serde::{de::DeserializeOwned, Deserialize};
    use toml::toml;

    use super::*;

    #[test]
    fn test_to_string() {
        Manifest {
            package: Some(Package {
                name: "package/name".to_string(),
                version: Version::parse("1.0.0").unwrap(),
                description: "test".to_string(),
                license: None,
                license_file: None,
                readme: None,
                repository: None,
                homepage: None,
                wasmer_extra_flags: None,
                disable_command_rename: false,
                rename_commands_to_raw_command_name: false,
                entrypoint: None,
                private: false,
            }),
            dependencies: HashMap::new(),
            modules: vec![Module {
                name: "test".to_string(),
                abi: Abi::Wasi,
                bindings: None,
                interfaces: None,
                kind: Some("https://webc.org/kind/wasi".to_string()),
                source: Path::new("test.wasm").to_path_buf(),
            }],
            commands: Vec::new(),
            fs: vec![
                ("a".to_string(), Path::new("/a").to_path_buf()),
                ("b".to_string(), Path::new("/b").to_path_buf()),
            ]
            .into_iter()
            .collect(),
        }
        .to_string()
        .unwrap();
    }

    #[test]
    fn interface_test() {
        let manifest_str = r#"
[package]
name = "test"
version = "0.0.0"
description = "This is a test package"
license = "MIT"

[[module]]
name = "mod"
source = "target/wasm32-wasi/release/mod.wasm"
interfaces = {"wasi" = "0.0.0-unstable"}

[[module]]
name = "mod-with-exports"
source = "target/wasm32-wasi/release/mod-with-exports.wasm"
bindings = { wit-exports = "exports.wit", wit-bindgen = "0.0.0" }

[[command]]
name = "command"
module = "mod"
"#;
        let manifest: Manifest = Manifest::parse(manifest_str).unwrap();
        let modules = &manifest.modules;
        assert_eq!(
            modules[0].interfaces.as_ref().unwrap().get("wasi"),
            Some(&"0.0.0-unstable".to_string())
        );

        assert_eq!(
            modules[1],
            Module {
                name: "mod-with-exports".to_string(),
                source: PathBuf::from("target/wasm32-wasi/release/mod-with-exports.wasm"),
                abi: Abi::None,
                kind: None,
                interfaces: None,
                bindings: Some(Bindings::Wit(WitBindings {
                    wit_exports: PathBuf::from("exports.wit"),
                    wit_bindgen: "0.0.0".parse().unwrap()
                })),
            },
        );
    }

    #[test]
    fn parse_wit_bindings() {
        let table = toml! {
            name = "..."
            source = "..."
            bindings = { wit-bindgen = "0.1.0", wit-exports = "./file.wit" }
        };

        let module = Module::deserialize(table).unwrap();

        assert_eq!(
            module.bindings.as_ref().unwrap(),
            &Bindings::Wit(WitBindings {
                wit_bindgen: "0.1.0".parse().unwrap(),
                wit_exports: PathBuf::from("./file.wit"),
            }),
        );
        assert_round_trippable(&module);
    }

    #[test]
    fn parse_wai_bindings() {
        let table = toml! {
            name = "..."
            source = "..."
            bindings = { wai-version = "0.1.0", exports = "./file.wai", imports = ["a.wai", "../b.wai"] }
        };

        let module = Module::deserialize(table).unwrap();

        assert_eq!(
            module.bindings.as_ref().unwrap(),
            &Bindings::Wai(WaiBindings {
                wai_version: "0.1.0".parse().unwrap(),
                exports: Some(PathBuf::from("./file.wai")),
                imports: vec![PathBuf::from("a.wai"), PathBuf::from("../b.wai")],
            }),
        );
        assert_round_trippable(&module);
    }

    #[track_caller]
    fn assert_round_trippable<T>(value: &T)
    where
        T: Serialize + DeserializeOwned + PartialEq + Debug,
    {
        let repr = toml::to_string(value).unwrap();
        let round_tripped: T = toml::from_str(&repr).unwrap();
        assert_eq!(
            round_tripped, *value,
            "The value should convert to/from TOML losslessly"
        );
    }

    #[test]
    fn imports_and_exports_are_optional_with_wai() {
        let table = toml! {
            name = "..."
            source = "..."
            bindings = { wai-version = "0.1.0" }
        };

        let module = Module::deserialize(table).unwrap();

        assert_eq!(
            module.bindings.as_ref().unwrap(),
            &Bindings::Wai(WaiBindings {
                wai_version: "0.1.0".parse().unwrap(),
                exports: None,
                imports: Vec::new(),
            }),
        );
        assert_round_trippable(&module);
    }

    #[test]
    fn ambiguous_bindings_table() {
        let table = toml! {
            wai-version = "0.2.0"
            wit-bindgen = "0.1.0"
        };

        let err = Bindings::deserialize(table).unwrap_err();

        assert_eq!(
            err.to_string(),
            "expected one of \"wit-bindgen\" or \"wai-version\" to be provided, but not both\n"
        );
    }

    #[test]
    fn bindings_table_that_is_neither_wit_nor_wai() {
        let table = toml! {
            wai-bindgen = "lol, this should have been wai-version"
            exports = "./file.wai"
        };

        let err = Bindings::deserialize(table).unwrap_err();

        assert_eq!(
            err.to_string(),
            "expected one of \"wit-bindgen\" or \"wai-version\" to be provided, but not both\n"
        );
    }

    #[test]
    fn command_v2_isnt_ambiguous_with_command_v1() {
        let src = r#"
[package]
name = "hotg-ai/sine"
version = "0.12.0"
description = "sine"

[dependencies]
"hotg-ai/train_test_split" = "0.12.1"
"hotg-ai/elastic_net" = "0.12.1"

[[module]] # This is the same as atoms
name = "sine"
kind = "tensorflow-SavedModel" # It can also be "wasm" (default)
source = "models/sine"

[[command]]
name = "run"
runner = "rune"
module = "sine"
annotations = { file = "Runefile.yml", kind = "yaml" }
"#;

        let manifest: Manifest = toml::from_str(src).unwrap();

        let commands = &manifest.commands;
        assert_eq!(commands.len(), 1);
        assert_eq!(
            commands[0],
            Command::V2(CommandV2 {
                name: "run".into(),
                module: "sine".parse().unwrap(),
                runner: "rune".into(),
                annotations: Some(CommandAnnotations::File(FileCommandAnnotations {
                    file: "Runefile.yml".into(),
                    kind: FileKind::Yaml,
                }))
            })
        );
    }

    #[test]
    fn get_manifest() {
        let wasmer_toml = toml! {
            [package]
            name = "test"
            version = "1.0.0"
            repository = "test.git"
            homepage = "test.com"
            description = "The best package."
        };
        let manifest: Manifest = wasmer_toml.try_into().unwrap();
        if let Some(package) = manifest.package {
            assert!(!package.disable_command_rename);
        }
    }

    #[test]
    fn parse_manifest_without_package_section() {
        let wasmer_toml = toml! {
            [[module]]
            name = "test-module"
            source = "data.wasm"
            abi = "wasi"
        };
        let manifest: Manifest = wasmer_toml.try_into().unwrap();
        assert!(manifest.package.is_none());
    }

    #[test]
    fn get_commands() {
        let wasmer_toml = toml! {
            [package]
            name = "test"
            version = "1.0.0"
            repository = "test.git"
            homepage = "test.com"
            description = "The best package."
            [[module]]
            name = "test-pkg"
            module = "target.wasm"
            source = "source.wasm"
            description = "description"
            interfaces = {"wasi" = "0.0.0-unstable"}
            [[command]]
            name = "foo"
            module = "test"
            [[command]]
            name = "baz"
            module = "test"
            main_args = "$@"
        };
        let manifest: Manifest = wasmer_toml.try_into().unwrap();
        let commands = &manifest.commands;
        assert_eq!(2, commands.len());
    }

    #[test]
    fn add_new_dependency() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let tmp_dir_path: &std::path::Path = tmp_dir.as_ref();
        let manifest_path = tmp_dir_path.join(MANIFEST_FILE_NAME);
        let wasmer_toml = toml! {
            [package]
            name = "_/test"
            version = "1.0.0"
            description = "description"
            [[module]]
            name = "test"
            source = "test.wasm"
            interfaces = {}
        };
        let toml_string = toml::to_string(&wasmer_toml).unwrap();
        std::fs::write(manifest_path, toml_string).unwrap();
        let mut manifest = Manifest::find_in_directory(tmp_dir).unwrap();

        let dependency_name = "dep_pkg";
        let dependency_version: VersionReq = "0.1.0".parse().unwrap();

        manifest.add_dependency(dependency_name.to_string(), dependency_version.clone());
        assert_eq!(1, manifest.dependencies.len());

        // adding the same dependency twice changes nothing
        manifest.add_dependency(dependency_name.to_string(), dependency_version);
        assert_eq!(1, manifest.dependencies.len());

        // adding a second different dependency will increase the count
        let dependency_name_2 = "dep_pkg_2";
        let dependency_version_2: VersionReq = "0.2.0".parse().unwrap();
        manifest.add_dependency(dependency_name_2.to_string(), dependency_version_2);
        assert_eq!(2, manifest.dependencies.len());
    }

    #[test]
    fn duplicate_modules_are_invalid() {
        let wasmer_toml = toml! {
            [package]
            name = "some/package"
            version = "0.0.0"
            description = ""
            [[module]]
            name = "test"
            source = "test.wasm"
            [[module]]
            name = "test"
            source = "test.wasm"
        };
        let manifest = Manifest::deserialize(wasmer_toml).unwrap();

        let error = manifest.validate().unwrap_err();

        assert_eq!(
            error,
            ValidationError::DuplicateModule {
                name: "test".to_string()
            }
        );
    }

    #[test]
    fn duplicate_commands_are_invalid() {
        let wasmer_toml = toml! {
            [package]
            name = "some/package"
            version = "0.0.0"
            description = ""
            [[module]]
            name = "test"
            source = "test.wasm"
            abi = "wasi"
            [[command]]
            name = "cmd"
            module = "test"
            [[command]]
            name = "cmd"
            module = "test"
        };
        let manifest = Manifest::deserialize(wasmer_toml).unwrap();

        let error = manifest.validate().unwrap_err();

        assert_eq!(
            error,
            ValidationError::DuplicateCommand {
                name: "cmd".to_string()
            }
        );
    }

    #[test]
    fn nonexistent_entrypoint() {
        let wasmer_toml = toml! {
            [package]
            name = "some/package"
            version = "0.0.0"
            description = ""
            entrypoint = "this-doesnt-exist"
            [[module]]
            name = "test"
            source = "test.wasm"
            abi = "wasi"
            [[command]]
            name = "cmd"
            module = "test"
        };
        let manifest = Manifest::deserialize(wasmer_toml).unwrap();

        let error = manifest.validate().unwrap_err();

        assert_eq!(
            error,
            ValidationError::InvalidEntrypoint {
                entrypoint: "this-doesnt-exist".to_string(),
                available_commands: vec!["cmd".to_string()]
            }
        );
    }

    #[test]
    fn command_with_nonexistent_module() {
        let wasmer_toml = toml! {
            [package]
            name = "some/package"
            version = "0.0.0"
            description = ""
            [[command]]
            name = "cmd"
            module = "this-doesnt-exist"
        };
        let manifest = Manifest::deserialize(wasmer_toml).unwrap();

        let error = manifest.validate().unwrap_err();

        assert_eq!(
            error,
            ValidationError::MissingModuleForCommand {
                command: "cmd".to_string(),
                module: "this-doesnt-exist".parse().unwrap()
            }
        );
    }

    #[test]
    fn use_builder_api_to_create_simplest_manifest() {
        let package =
            Package::builder("my/package", "1.0.0".parse().unwrap(), "My awesome package")
                .build()
                .unwrap();
        let manifest = Manifest::builder(package).build().unwrap();

        manifest.validate().unwrap();
    }

    #[test]
    fn deserialize_command_referring_to_module_from_dependency() {
        let wasmer_toml = toml! {
            [package]
            name = "some/package"
            version = "0.0.0"
            description = ""

            [dependencies]
            dep = "1.2.3"

            [[command]]
            name = "cmd"
            module = "dep:module"
        };
        let manifest = Manifest::deserialize(wasmer_toml).unwrap();

        let command = manifest
            .commands
            .iter()
            .find(|cmd| cmd.get_name() == "cmd")
            .unwrap();

        assert_eq!(
            command.get_module(),
            &ModuleReference::Dependency {
                dependency: "dep".to_string(),
                module: "module".to_string()
            }
        );
    }

    #[test]
    fn command_with_module_from_nonexistent_dependency() {
        let wasmer_toml = toml! {
            [package]
            name = "some/package"
            version = "0.0.0"
            description = ""
            [[command]]
            name = "cmd"
            module = "dep:module"
        };
        let manifest = Manifest::deserialize(wasmer_toml).unwrap();

        let error = manifest.validate().unwrap_err();

        assert_eq!(
            error,
            ValidationError::MissingDependency {
                command: "cmd".to_string(),
                dependency: "dep".to_string(),
                module_ref: ModuleReference::Dependency {
                    dependency: "dep".to_string(),
                    module: "module".to_string()
                }
            }
        );
    }

    #[test]
    fn round_trip_dependency_module_ref() {
        let original = ModuleReference::Dependency {
            dependency: "my/dep".to_string(),
            module: "module".to_string(),
        };

        let repr = original.to_string();
        let round_tripped: ModuleReference = repr.parse().unwrap();

        assert_eq!(round_tripped, original);
    }
}
