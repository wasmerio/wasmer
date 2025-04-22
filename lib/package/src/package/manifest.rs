use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use ciborium::{cbor, Value};
use semver::VersionReq;
use sha2::Digest;
use shared_buffer::{MmapError, OwnedBuffer};
use url::Url;
#[allow(deprecated)]
use wasmer_config::package::{CommandV1, CommandV2, Manifest as WasmerManifest, Package};
use wasmer_config::package::{SuggestedCompilerOptimizations, UserAnnotations};
use webc::{
    indexmap::{self, IndexMap},
    metadata::AtomSignature,
    sanitize_path,
};

use crate::utils::features_to_wasm_annotations;

use webc::metadata::{
    annotations::{
        Atom as AtomAnnotation, FileSystemMapping, FileSystemMappings, VolumeSpecificPath, Wapm,
        Wasi,
    },
    Atom, Binding, Command, Manifest as WebcManifest, UrlOrManifest, WaiBindings, WitBindings,
};

use super::{FsVolume, Strictness};

const METADATA_VOLUME: &str = FsVolume::METADATA;

/// Errors that may occur when converting from a [`wasmer_config::package::Manifest`] to
/// a [`crate::metadata::Manifest`].
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ManifestError {
    /// A dependency specification had a syntax error.
    #[error("The dependency, \"{_0}\", isn't in the \"namespace/name\" format")]
    InvalidDependency(String),
    /// Unable to serialize an annotation.
    #[error("Unable to serialize the \"{key}\" annotation")]
    SerializeCborAnnotation {
        /// Which annotation was being serialized?
        key: String,
        /// The underlying error.
        #[source]
        error: ciborium::value::Error,
    },
    /// Specified an unknown atom kind.
    #[error("Unknown atom kind, \"{_0}\"")]
    UnknownAtomKind(String),
    /// A module was specified more than once.
    #[error("Duplicate module, \"{_0}\"")]
    DuplicateModule(String),
    /// Unable to read a module's `source`.
    #[error("Unable to read the \"{module}\" module's file from \"{}\"", path.display())]
    ReadAtomFile {
        /// The name of the module.
        module: String,
        /// The path that was read.
        path: PathBuf,
        /// The underlying error.
        #[source]
        error: std::io::Error,
    },
    /// A command was specified more than once.
    #[error("Duplicate command, \"{_0}\"")]
    DuplicateCommand(String),
    /// An unknown runner kind was specified.
    #[error("Unknown runner kind, \"{_0}\"")]
    UnknownRunnerKind(String),
    /// An error occurred while merging user-defined annotations in with
    /// automatically generated ones.
    #[error("Unable to merge in user-defined \"{key}\" annotations for the \"{command}\" command")]
    #[non_exhaustive]
    MergeAnnotations {
        /// The command annotations were being merged for.
        command: String,
        /// The annotation that was being merged.
        key: String,
    },
    /// A command uses a non-existent module.
    #[error("The \"{command}\" command uses a non-existent module, \"{module}\"")]
    InvalidModuleReference {
        /// The command name.
        command: String,
        /// The module name.
        module: String,
    },
    /// A command references a module from an undeclared dependency.
    #[error("The \"{command}\" command references the undeclared dependency \"{dependency}\"")]
    UndeclaredCommandDependency {
        /// The command name.
        command: String,
        /// The dependency name.
        dependency: String,
    },
    /// Unable to deserialize custom annotations from the `wasmer.toml`
    /// manifest.
    #[error("Unable to deserialize custom annotations from the wasmer.toml manifest")]
    WasmerTomlAnnotations {
        /// The underlying error.
        #[source]
        error: Box<dyn std::error::Error + Send + Sync>,
    },
    /// The `wasmer.toml` file references a file outside of its base directory.
    #[error("\"{}\" is outside of \"{}\"", path.display(), base_dir.display())]
    OutsideBaseDirectory {
        /// The file that was referenced.
        path: PathBuf,
        /// The base directory.
        base_dir: PathBuf,
    },
    /// The manifest references a file that doesn't exist.
    #[error("The \"{}\" doesn't exist (base dir: {})", path.display(), base_dir.display())]
    MissingFile {
        /// The file that was referenced.
        path: PathBuf,
        /// The base directory.
        base_dir: PathBuf,
    },
    /// File based commands are not supported for in-memory package creation
    #[error("File based commands are not supported for in-memory package creation")]
    FileNotSupported,
}

/// take a `wasmer.toml` manifest and convert it to the `*.webc` equivalent.
pub(crate) fn wasmer_manifest_to_webc(
    manifest: &WasmerManifest,
    base_dir: &Path,
    strictness: Strictness,
) -> Result<(WebcManifest, BTreeMap<String, OwnedBuffer>), ManifestError> {
    let use_map = transform_dependencies(&manifest.dependencies)?;

    // Note: We need to clone the [fs] table because the wasmer-toml crate has
    // already upgraded to indexmap v2.0, but the webc crate needs to stay at
    // 1.9.2 for backwards compatibility reasons.
    let fs: IndexMap<String, PathBuf> = manifest.fs.clone().into_iter().collect();

    let package =
        transform_package_annotations(manifest.package.as_ref(), &fs, base_dir, strictness)?;
    let (atoms, atom_files) = transform_atoms(manifest, base_dir)?;
    let commands = transform_commands(manifest, base_dir)?;
    let bindings = transform_bindings(manifest, base_dir)?;

    let manifest = WebcManifest {
        origin: None,
        use_map,
        package,
        atoms,
        commands,
        bindings,
        entrypoint: entrypoint(manifest),
    };

    Ok((manifest, atom_files))
}

/// take a `wasmer.toml` manifest and convert it to the `*.webc` equivalent.
pub(crate) fn in_memory_wasmer_manifest_to_webc(
    manifest: &WasmerManifest,
    atoms: &BTreeMap<String, (Option<String>, OwnedBuffer, Option<&UserAnnotations>)>,
) -> Result<(WebcManifest, BTreeMap<String, OwnedBuffer>), ManifestError> {
    let use_map = transform_dependencies(&manifest.dependencies)?;

    // Note: We need to clone the [fs] table because the wasmer-toml crate has
    // already upgraded to indexmap v2.0, but the webc crate needs to stay at
    // 1.9.2 for backwards compatibility reasons.
    let fs: IndexMap<String, PathBuf> = manifest.fs.clone().into_iter().collect();

    let package = transform_in_memory_package_annotations(manifest.package.as_ref(), &fs)?;
    let (atoms, atom_files) = transform_in_memory_atoms(atoms)?;
    let commands = transform_in_memory_commands(manifest)?;
    let bindings = transform_in_memory_bindings(manifest)?;

    let manifest = WebcManifest {
        origin: None,
        use_map,
        package,
        atoms,
        commands,
        bindings,
        entrypoint: entrypoint(manifest),
    };

    Ok((manifest, atom_files))
}

fn transform_package_annotations(
    package: Option<&wasmer_config::package::Package>,
    fs: &IndexMap<String, PathBuf>,
    base_dir: &Path,
    strictness: Strictness,
) -> Result<IndexMap<String, Value>, ManifestError> {
    transform_package_annotations_shared(package, fs, |package| {
        transform_package_meta_to_annotations(package, base_dir, strictness)
    })
}

fn transform_in_memory_package_annotations(
    package: Option<&wasmer_config::package::Package>,
    fs: &IndexMap<String, PathBuf>,
) -> Result<IndexMap<String, Value>, ManifestError> {
    transform_package_annotations_shared(package, fs, |package| {
        transform_in_memory_package_meta_to_annotations(package)
    })
}

fn transform_package_annotations_shared(
    package: Option<&wasmer_config::package::Package>,
    fs: &IndexMap<String, PathBuf>,
    transform_package_meta_to_annotations: impl Fn(&Package) -> Result<Wapm, ManifestError>,
) -> Result<IndexMap<String, Value>, ManifestError> {
    let mut annotations = IndexMap::new();

    if let Some(wasmer_package) = package {
        let wapm = transform_package_meta_to_annotations(wasmer_package)?;
        insert_annotation(&mut annotations, Wapm::KEY, wapm)?;
    }

    let fs = get_fs_table(fs);

    if !fs.is_empty() {
        insert_annotation(&mut annotations, FileSystemMappings::KEY, fs)?;
    }

    Ok(annotations)
}

fn transform_dependencies(
    original_dependencies: &IndexMap<String, VersionReq>,
) -> Result<IndexMap<String, UrlOrManifest>, ManifestError> {
    let mut dependencies = IndexMap::new();

    for (dep, version) in original_dependencies {
        let (namespace, package_name) = extract_dependency_parts(dep)
            .ok_or_else(|| ManifestError::InvalidDependency(dep.clone()))?;

        // Note: the wasmer.toml format forces you to go through a registry for
        // all dependencies. There's no way to specify a URL-based dependency.
        let dependency_specifier =
            UrlOrManifest::RegistryDependentUrl(format!("{namespace}/{package_name}@{version}"));

        dependencies.insert(dep.clone(), dependency_specifier);
    }

    Ok(dependencies)
}

fn extract_dependency_parts(dep: &str) -> Option<(&str, &str)> {
    let (namespace, package_name) = dep.split_once('/')?;

    fn invalid_char(c: char) -> bool {
        !matches!(c, 'a'..='z' | 'A'..='Z' | '_' | '-' | '0'..='9')
    }

    if namespace.contains(invalid_char) || package_name.contains(invalid_char) {
        None
    } else {
        Some((namespace, package_name))
    }
}

type Atoms = BTreeMap<String, OwnedBuffer>;

fn transform_atoms(
    manifest: &WasmerManifest,
    base_dir: &Path,
) -> Result<(IndexMap<String, Atom>, Atoms), ManifestError> {
    let mut atom_entries = BTreeMap::new();

    for module in &manifest.modules {
        let name = &module.name;
        let path = base_dir.join(&module.source);
        let file = open_file(&path).map_err(|error| ManifestError::ReadAtomFile {
            module: name.clone(),
            path,
            error,
        })?;

        atom_entries.insert(
            name.clone(),
            (module.kind.clone(), file, module.annotations.as_ref()),
        );
    }

    transform_atoms_shared(&atom_entries)
}

fn transform_in_memory_atoms(
    atoms: &BTreeMap<String, (Option<String>, OwnedBuffer, Option<&UserAnnotations>)>,
) -> Result<(IndexMap<String, Atom>, Atoms), ManifestError> {
    transform_atoms_shared(atoms)
}

fn transform_atoms_shared(
    atoms: &BTreeMap<String, (Option<String>, OwnedBuffer, Option<&UserAnnotations>)>,
) -> Result<(IndexMap<String, Atom>, Atoms), ManifestError> {
    let mut atom_files = BTreeMap::new();
    let mut metadata = IndexMap::new();

    for (name, (kind, content, misc_annotations)) in atoms.iter() {
        // Create atom with annotations including Wasm features if available
        let mut annotations = IndexMap::new();
        if let Some(misc_annotations) = misc_annotations {
            if let Some(pass_params) = misc_annotations
                .suggested_compiler_optimizations
                .pass_params
            {
                annotations.insert(
                    SuggestedCompilerOptimizations::KEY.to_string(),
                    cbor!({"pass_params" => pass_params}).unwrap(),
                );
            }
        }

        // Detect required WebAssembly features by analyzing the module binary
        let features_result = wasmer_types::Features::detect_from_wasm(content);

        if let Ok(features) = features_result {
            // Convert wasmer_types::Features to webc::metadata::annotations::Wasm
            let feature_strings = features_to_wasm_annotations(&features);

            // Only create annotation if we detected features
            if !feature_strings.is_empty() {
                let wasm = webc::metadata::annotations::Wasm::new(feature_strings);
                match ciborium::value::Value::serialized(&wasm) {
                    Ok(wasm_value) => {
                        annotations.insert(
                            webc::metadata::annotations::Wasm::KEY.to_string(),
                            wasm_value,
                        );
                    }
                    Err(e) => {
                        eprintln!("Failed to serialize wasm features: {e}");
                    }
                }
            }
        }

        let atom = Atom {
            kind: atom_kind(kind.as_ref().map(|s| s.as_str()))?,
            signature: atom_signature(content),
            annotations,
        };

        if metadata.contains_key(name) {
            return Err(ManifestError::DuplicateModule(name.clone()));
        }

        metadata.insert(name.clone(), atom);
        atom_files.insert(name.clone(), content.clone());
    }

    Ok((metadata, atom_files))
}

fn atom_signature(atom: &[u8]) -> String {
    let hash: [u8; 32] = sha2::Sha256::digest(atom).into();
    AtomSignature::Sha256(hash).to_string()
}

/// Map the "kind" field in a `[module]` to the corresponding URI.
fn atom_kind(kind: Option<&str>) -> Result<Url, ManifestError> {
    const WASM_ATOM_KIND: &str = "https://webc.org/kind/wasm";
    const TENSORFLOW_SAVED_MODEL_KIND: &str = "https://webc.org/kind/tensorflow-SavedModel";

    let url = match kind {
        Some("wasm") | None => WASM_ATOM_KIND.parse().expect("Should never fail"),
        Some("tensorflow-SavedModel") => TENSORFLOW_SAVED_MODEL_KIND
            .parse()
            .expect("Should never fail"),
        Some(other) => {
            if let Ok(url) = Url::parse(other) {
                // if it is a valid URL, pass that through as-is
                url
            } else {
                return Err(ManifestError::UnknownAtomKind(other.to_string()));
            }
        }
    };

    Ok(url)
}

/// Try to open a file, preferring mmap and falling back to [`std::fs::read()`]
/// if mapping fails.
fn open_file(path: &Path) -> Result<OwnedBuffer, std::io::Error> {
    match OwnedBuffer::mmap(path) {
        Ok(b) => return Ok(b),
        Err(MmapError::Map(_)) => {
            // Unable to mmap the atom file. Falling back to std::fs::read()
        }
        Err(MmapError::FileOpen { error, .. }) => {
            return Err(error);
        }
    }

    let bytes = std::fs::read(path)?;

    Ok(OwnedBuffer::from_bytes(bytes))
}

fn insert_annotation(
    annotations: &mut IndexMap<String, ciborium::Value>,
    key: impl Into<String>,
    value: impl serde::Serialize,
) -> Result<(), ManifestError> {
    let key = key.into();

    match ciborium::value::Value::serialized(&value) {
        Ok(value) => {
            annotations.insert(key, value);
            Ok(())
        }
        Err(error) => Err(ManifestError::SerializeCborAnnotation { key, error }),
    }
}

fn get_fs_table(fs: &IndexMap<String, PathBuf>) -> FileSystemMappings {
    if fs.is_empty() {
        return FileSystemMappings::default();
    }

    // When wapm-targz-to-pirita creates the final webc all files will be
    // merged into one "atom" volume, but we want to map each directory
    // separately.
    let mut entries = Vec::new();
    for (guest, host) in fs {
        let volume_name = host
            .to_str()
            .expect("failed to convert path to string")
            .to_string();

        let volume_name = sanitize_path(volume_name);

        let mapping = FileSystemMapping {
            from: None,
            volume_name,
            host_path: None,
            mount_path: sanitize_path(guest),
        };
        entries.push(mapping);
    }

    FileSystemMappings(entries)
}

fn transform_package_meta_to_annotations(
    package: &wasmer_config::package::Package,
    base_dir: &Path,
    strictness: Strictness,
) -> Result<Wapm, ManifestError> {
    fn metadata_file(
        path: Option<&PathBuf>,
        base_dir: &Path,
        strictness: Strictness,
    ) -> Result<Option<VolumeSpecificPath>, ManifestError> {
        let path = match path {
            Some(p) => p,
            None => return Ok(None),
        };

        let absolute_path = base_dir.join(path);

        // Touch the file to make sure it actually exists
        if !absolute_path.exists() {
            match strictness.missing_file(path, base_dir) {
                Ok(_) => return Ok(None),
                Err(e) => {
                    return Err(e);
                }
            }
        }

        match base_dir.join(path).strip_prefix(base_dir) {
            Ok(without_prefix) => Ok(Some(VolumeSpecificPath {
                volume: METADATA_VOLUME.to_string(),
                path: sanitize_path(without_prefix),
            })),
            Err(_) => match strictness.outside_base_directory(path, base_dir) {
                Ok(_) => Ok(None),
                Err(e) => Err(e),
            },
        }
    }

    transform_package_meta_to_annotations_shared(package, |path| {
        metadata_file(path, base_dir, strictness)
    })
}

fn transform_in_memory_package_meta_to_annotations(
    package: &wasmer_config::package::Package,
) -> Result<Wapm, ManifestError> {
    transform_package_meta_to_annotations_shared(package, |path| {
        Ok(path.map(|readme_file| VolumeSpecificPath {
            volume: METADATA_VOLUME.to_string(),
            path: sanitize_path(readme_file),
        }))
    })
}

fn transform_package_meta_to_annotations_shared(
    package: &wasmer_config::package::Package,
    volume_specific_path: impl Fn(Option<&PathBuf>) -> Result<Option<VolumeSpecificPath>, ManifestError>,
) -> Result<Wapm, ManifestError> {
    let mut wapm = Wapm::new(
        package.name.clone(),
        package.version.clone().map(|v| v.to_string()),
        package.description.clone(),
    );

    wapm.license = package.license.clone();
    wapm.license_file = volume_specific_path(package.license_file.as_ref())?;
    wapm.readme = volume_specific_path(package.readme.as_ref())?;
    wapm.repository = package.repository.clone();
    wapm.homepage = package.homepage.clone();
    wapm.private = package.private;

    Ok(wapm)
}

fn transform_commands(
    manifest: &WasmerManifest,
    base_dir: &Path,
) -> Result<IndexMap<String, Command>, ManifestError> {
    trasform_commands_shared(
        manifest,
        |cmd| transform_command_v1(cmd, manifest),
        |cmd| transform_command_v2(cmd, base_dir),
    )
}

fn transform_in_memory_commands(
    manifest: &WasmerManifest,
) -> Result<IndexMap<String, Command>, ManifestError> {
    trasform_commands_shared(
        manifest,
        |cmd| transform_command_v1(cmd, manifest),
        transform_in_memory_command_v2,
    )
}

#[allow(deprecated)]
fn trasform_commands_shared(
    manifest: &WasmerManifest,
    transform_command_v1: impl Fn(&CommandV1) -> Result<Command, ManifestError>,
    transform_command_v2: impl Fn(&CommandV2) -> Result<Command, ManifestError>,
) -> Result<IndexMap<String, Command>, ManifestError> {
    let mut commands = IndexMap::new();

    for command in &manifest.commands {
        let cmd = match command {
            wasmer_config::package::Command::V1(cmd) => transform_command_v1(cmd)?,
            wasmer_config::package::Command::V2(cmd) => transform_command_v2(cmd)?,
        };

        // If a command uses a module from a dependency, then ensure that
        // the dependency is declared.
        match command.get_module() {
            wasmer_config::package::ModuleReference::CurrentPackage { .. } => {}
            wasmer_config::package::ModuleReference::Dependency { dependency, .. } => {
                if !manifest.dependencies.contains_key(dependency) {
                    return Err(ManifestError::UndeclaredCommandDependency {
                        command: command.get_name().to_string(),
                        dependency: dependency.to_string(),
                    });
                }
            }
        }

        match commands.entry(command.get_name().to_string()) {
            indexmap::map::Entry::Occupied(_) => {
                return Err(ManifestError::DuplicateCommand(
                    command.get_name().to_string(),
                ));
            }
            indexmap::map::Entry::Vacant(entry) => {
                entry.insert(cmd);
            }
        }
    }

    Ok(commands)
}

#[allow(deprecated)]
fn transform_command_v1(
    cmd: &wasmer_config::package::CommandV1,
    manifest: &WasmerManifest,
) -> Result<Command, ManifestError> {
    // Note: a key difference between CommandV1 and CommandV2 is that v1 uses
    // a module's "abi" field to figure out which runner to use, whereas v2 has
    // a dedicated "runner" field and ignores module.abi.
    let runner = match &cmd.module {
        wasmer_config::package::ModuleReference::CurrentPackage { module } => {
            let module = manifest
                .modules
                .iter()
                .find(|m| m.name == module.as_str())
                .ok_or_else(|| ManifestError::InvalidModuleReference {
                    command: cmd.name.clone(),
                    module: cmd.module.to_string(),
                })?;

            RunnerKind::from_name(module.abi.to_str())?
        }
        wasmer_config::package::ModuleReference::Dependency { .. } => {
            // Note: We don't have any visibility into dependencies (this code
            // doesn't do resolution), so we blindly assume it's a WASI command.
            // That should be fine because people shouldn't use the CommandV1
            // syntax any more.
            RunnerKind::Wasi
        }
    };

    let mut annotations = IndexMap::new();
    // Splitting by whitespace isn't really correct, but proper shell splitting
    // would require a dependency and CommandV1 won't be used any more, anyway.
    let main_args = cmd
        .main_args
        .as_deref()
        .map(|args| args.split_whitespace().map(String::from).collect());
    runner.runner_specific_annotations(
        &mut annotations,
        &cmd.module,
        cmd.package.clone(),
        main_args,
    )?;

    Ok(Command {
        runner: runner.uri().to_string(),
        annotations,
    })
}

fn transform_command_v2(
    cmd: &wasmer_config::package::CommandV2,
    base_dir: &Path,
) -> Result<Command, ManifestError> {
    transform_command_v2_shared(cmd, || {
        cmd.get_annotations(base_dir)
            .map_err(|error| ManifestError::WasmerTomlAnnotations {
                error: error.into(),
            })
    })
}

fn transform_in_memory_command_v2(
    cmd: &wasmer_config::package::CommandV2,
) -> Result<Command, ManifestError> {
    transform_command_v2_shared(cmd, || {
        cmd.annotations
            .as_ref()
            .map(|a| match a {
                wasmer_config::package::CommandAnnotations::File(_) => {
                    Err(ManifestError::FileNotSupported)
                }
                wasmer_config::package::CommandAnnotations::Raw(v) => Ok(toml_to_cbor_value(v)),
            })
            .transpose()
    })
}

fn transform_command_v2_shared(
    cmd: &wasmer_config::package::CommandV2,
    custom_annotations: impl Fn() -> Result<Option<Value>, ManifestError>,
) -> Result<Command, ManifestError> {
    let runner = RunnerKind::from_name(&cmd.runner)?;
    let mut annotations = IndexMap::new();

    runner.runner_specific_annotations(&mut annotations, &cmd.module, None, None)?;

    let custom_annotations = custom_annotations()?;

    if let Some(ciborium::Value::Map(custom_annotations)) = custom_annotations {
        for (key, value) in custom_annotations {
            if let ciborium::Value::Text(key) = key {
                match annotations.entry(key) {
                    indexmap::map::Entry::Occupied(mut entry) => {
                        merge_cbor(entry.get_mut(), value).map_err(|_| {
                            ManifestError::MergeAnnotations {
                                command: cmd.name.clone(),
                                key: entry.key().clone(),
                            }
                        })?;
                    }
                    indexmap::map::Entry::Vacant(entry) => {
                        entry.insert(value);
                    }
                }
            }
        }
    }

    Ok(Command {
        runner: runner.uri().to_string(),
        annotations,
    })
}

fn toml_to_cbor_value(val: &toml::value::Value) -> ciborium::Value {
    match val {
        toml::Value::String(s) => ciborium::Value::Text(s.clone()),
        toml::Value::Integer(i) => ciborium::Value::Integer(ciborium::value::Integer::from(*i)),
        toml::Value::Float(f) => ciborium::Value::Float(*f),
        toml::Value::Boolean(b) => ciborium::Value::Bool(*b),
        toml::Value::Datetime(d) => ciborium::Value::Text(format!("{d}")),
        toml::Value::Array(sq) => {
            ciborium::Value::Array(sq.iter().map(toml_to_cbor_value).collect())
        }
        toml::Value::Table(m) => ciborium::Value::Map(
            m.iter()
                .map(|(k, v)| (ciborium::Value::Text(k.clone()), toml_to_cbor_value(v)))
                .collect(),
        ),
    }
}

fn merge_cbor(original: &mut Value, addition: Value) -> Result<(), ()> {
    match (original, addition) {
        (Value::Map(left), Value::Map(right)) => {
            for (k, v) in right {
                if let Some(entry) = left.iter_mut().find(|lk| lk.0 == k) {
                    merge_cbor(&mut entry.1, v)?;
                } else {
                    left.push((k, v));
                }
            }
        }
        (Value::Array(left), Value::Array(right)) => {
            left.extend(right);
        }
        // Primitives that have the same values are fine
        (Value::Bool(left), Value::Bool(right)) if *left == right => {}
        (Value::Bytes(left), Value::Bytes(right)) if *left == right => {}
        (Value::Float(left), Value::Float(right)) if *left == right => {}
        (Value::Integer(left), Value::Integer(right)) if *left == right => {}
        (Value::Text(left), Value::Text(right)) if *left == right => {}
        // null can be overwritten
        (original @ Value::Null, value) => {
            *original = value;
        }
        (_original, Value::Null) => {}
        // Oh well, we tried...
        (_left, _right) => {
            return Err(());
        }
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
enum RunnerKind {
    Wasi,
    Wcgi,
    Wasm4,
    Other(Url),
}

impl RunnerKind {
    fn from_name(name: &str) -> Result<Self, ManifestError> {
        match name {
            "wasi" | "wasi@unstable_" | webc::metadata::annotations::WASI_RUNNER_URI => {
                Ok(RunnerKind::Wasi)
            }
            "generic" => {
                // This is what you get with a CommandV1 and abi = "none"
                Ok(RunnerKind::Wasi)
            }
            "wcgi" | webc::metadata::annotations::WCGI_RUNNER_URI => Ok(RunnerKind::Wcgi),
            "wasm4" | webc::metadata::annotations::WASM4_RUNNER_URI => Ok(RunnerKind::Wasm4),
            other => {
                if let Ok(other) = Url::parse(other) {
                    Ok(RunnerKind::Other(other))
                } else if let Ok(other) = format!("https://webc.org/runner/{other}").parse() {
                    // fall back to something under webc.org
                    Ok(RunnerKind::Other(other))
                } else {
                    Err(ManifestError::UnknownRunnerKind(other.to_string()))
                }
            }
        }
    }

    fn uri(&self) -> &str {
        match self {
            RunnerKind::Wasi => webc::metadata::annotations::WASI_RUNNER_URI,
            RunnerKind::Wcgi => webc::metadata::annotations::WCGI_RUNNER_URI,
            RunnerKind::Wasm4 => webc::metadata::annotations::WASM4_RUNNER_URI,
            RunnerKind::Other(other) => other.as_str(),
        }
    }

    #[allow(deprecated)]
    fn runner_specific_annotations(
        &self,
        annotations: &mut IndexMap<String, Value>,
        module: &wasmer_config::package::ModuleReference,
        package: Option<String>,
        main_args: Option<Vec<String>>,
    ) -> Result<(), ManifestError> {
        let atom_annotation = match module {
            wasmer_config::package::ModuleReference::CurrentPackage { module } => {
                AtomAnnotation::new(module, None)
            }
            wasmer_config::package::ModuleReference::Dependency { dependency, module } => {
                AtomAnnotation::new(module, dependency.to_string())
            }
        };
        insert_annotation(annotations, AtomAnnotation::KEY, atom_annotation)?;

        match self {
            RunnerKind::Wasi | RunnerKind::Wcgi => {
                let mut wasi = Wasi::new(module.to_string());
                wasi.main_args = main_args;
                wasi.package = package;
                insert_annotation(annotations, Wasi::KEY, wasi)?;
            }
            RunnerKind::Wasm4 | RunnerKind::Other(_) => {
                // No extra annotations to add
            }
        }

        Ok(())
    }
}

/// Infer the package's entrypoint.
fn entrypoint(manifest: &WasmerManifest) -> Option<String> {
    // check if manifest.package is none
    if let Some(package) = &manifest.package {
        if let Some(entrypoint) = &package.entrypoint {
            return Some(entrypoint.clone());
        }
    }

    if let [only_command] = manifest.commands.as_slice() {
        // For convenience (and to stay compatible with old docs), if there is
        // only one command we'll use that as the entrypoint
        return Some(only_command.get_name().to_string());
    }

    None
}

fn transform_bindings(
    manifest: &WasmerManifest,
    base_dir: &Path,
) -> Result<Vec<Binding>, ManifestError> {
    transform_bindings_shared(
        manifest,
        |wit, module| transform_wit_bindings(wit, module, base_dir),
        |wit, module| transform_wai_bindings(wit, module, base_dir),
    )
}

fn transform_in_memory_bindings(manifest: &WasmerManifest) -> Result<Vec<Binding>, ManifestError> {
    transform_bindings_shared(
        manifest,
        transform_in_memory_wit_bindings,
        transform_in_memory_wai_bindings,
    )
}

fn transform_bindings_shared(
    manifest: &WasmerManifest,
    wit_binding: impl Fn(
        &wasmer_config::package::WitBindings,
        &wasmer_config::package::Module,
    ) -> Result<Binding, ManifestError>,
    wai_binding: impl Fn(
        &wasmer_config::package::WaiBindings,
        &wasmer_config::package::Module,
    ) -> Result<Binding, ManifestError>,
) -> Result<Vec<Binding>, ManifestError> {
    let mut bindings = Vec::new();

    for module in &manifest.modules {
        let b = match &module.bindings {
            Some(wasmer_config::package::Bindings::Wit(wit)) => wit_binding(wit, module)?,
            Some(wasmer_config::package::Bindings::Wai(wai)) => wai_binding(wai, module)?,
            None => continue,
        };
        bindings.push(b);
    }

    Ok(bindings)
}

fn transform_wai_bindings(
    wai: &wasmer_config::package::WaiBindings,
    module: &wasmer_config::package::Module,
    base_dir: &Path,
) -> Result<Binding, ManifestError> {
    transform_wai_bindings_shared(wai, module, |path| metadata_volume_uri(path, base_dir))
}

fn transform_in_memory_wai_bindings(
    wai: &wasmer_config::package::WaiBindings,
    module: &wasmer_config::package::Module,
) -> Result<Binding, ManifestError> {
    transform_wai_bindings_shared(wai, module, |path| {
        Ok(format!("{METADATA_VOLUME}:/{}", sanitize_path(path)))
    })
}

fn transform_wai_bindings_shared(
    wai: &wasmer_config::package::WaiBindings,
    module: &wasmer_config::package::Module,
    metadata_volume_path: impl Fn(&PathBuf) -> Result<String, ManifestError>,
) -> Result<Binding, ManifestError> {
    let wasmer_config::package::WaiBindings {
        wai_version,
        exports,
        imports,
    } = wai;

    let bindings = WaiBindings {
        exports: exports.as_ref().map(&metadata_volume_path).transpose()?,
        module: module.name.clone(),
        imports: imports
            .iter()
            .map(metadata_volume_path)
            .collect::<Result<Vec<_>, ManifestError>>()?,
    };
    let mut annotations = IndexMap::new();
    insert_annotation(&mut annotations, "wai", bindings)?;

    Ok(Binding {
        name: "library-bindings".to_string(),
        kind: format!("wai@{wai_version}"),
        annotations: Value::Map(
            annotations
                .into_iter()
                .map(|(k, v)| (Value::Text(k), v))
                .collect(),
        ),
    })
}

fn metadata_volume_uri(path: &Path, base_dir: &Path) -> Result<String, ManifestError> {
    make_relative_path(path, base_dir)
        .map(sanitize_path)
        .map(|p| format!("{METADATA_VOLUME}:/{p}"))
}

fn transform_wit_bindings(
    wit: &wasmer_config::package::WitBindings,
    module: &wasmer_config::package::Module,
    base_dir: &Path,
) -> Result<Binding, ManifestError> {
    transform_wit_bindings_shared(wit, module, |path| metadata_volume_uri(path, base_dir))
}

fn transform_in_memory_wit_bindings(
    wit: &wasmer_config::package::WitBindings,
    module: &wasmer_config::package::Module,
) -> Result<Binding, ManifestError> {
    transform_wit_bindings_shared(wit, module, |path| {
        Ok(format!("{METADATA_VOLUME}:/{}", sanitize_path(path)))
    })
}

fn transform_wit_bindings_shared(
    wit: &wasmer_config::package::WitBindings,
    module: &wasmer_config::package::Module,
    metadata_volume_path: impl Fn(&PathBuf) -> Result<String, ManifestError>,
) -> Result<Binding, ManifestError> {
    let wasmer_config::package::WitBindings {
        wit_bindgen,
        wit_exports,
    } = wit;

    let bindings = WitBindings {
        exports: metadata_volume_path(wit_exports)?,
        module: module.name.clone(),
    };
    let mut annotations = IndexMap::new();
    insert_annotation(&mut annotations, "wit", bindings)?;

    Ok(Binding {
        name: "library-bindings".to_string(),
        kind: format!("wit@{wit_bindgen}"),
        annotations: Value::Map(
            annotations
                .into_iter()
                .map(|(k, v)| (Value::Text(k), v))
                .collect(),
        ),
    })
}

/// Resolve an item relative to the base directory, returning an error if the
/// file lies outside of it.
fn make_relative_path(path: &Path, base_dir: &Path) -> Result<PathBuf, ManifestError> {
    let absolute_path = base_dir.join(path);

    match absolute_path.strip_prefix(base_dir) {
        Ok(p) => Ok(p.into()),
        Err(_) => Err(ManifestError::OutsideBaseDirectory {
            path: absolute_path,
            base_dir: base_dir.to_path_buf(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;
    use webc::metadata::annotations::Wasi;

    use super::*;

    #[test]
    fn custom_annotations_are_copied_across_verbatim() {
        let temp = TempDir::new().unwrap();
        let wasmer_toml = r#"
        [package]
        name = "test"
        version = "0.0.0"
        description = "asdf"

        [[module]]
        name = "module"
        source = "file.wasm"
        abi = "wasi"

        [[command]]
        name = "command"
        module = "module"
        runner = "asdf"
        annotations = { first = 42, second = ["a", "b"] }
        "#;
        let manifest: WasmerManifest = toml::from_str(wasmer_toml).unwrap();
        std::fs::write(temp.path().join("file.wasm"), b"\0asm...").unwrap();

        let (transformed, _) =
            wasmer_manifest_to_webc(&manifest, temp.path(), Strictness::Strict).unwrap();

        let command = &transformed.commands["command"];
        assert_eq!(command.annotation::<u32>("first").unwrap(), Some(42));
        assert_eq!(command.annotation::<String>("non-existent").unwrap(), None);
        insta::with_settings! {
            { description => wasmer_toml },
            { insta::assert_yaml_snapshot!(&transformed); }
        }
    }

    #[test]
    fn transform_empty_manifest() {
        let temp = TempDir::new().unwrap();
        let wasmer_toml = r#"
            [package]
            name = "some/package"
            version = "0.0.0"
            description = "My awesome package"
        "#;
        let manifest: WasmerManifest = toml::from_str(wasmer_toml).unwrap();

        let (transformed, atoms) =
            wasmer_manifest_to_webc(&manifest, temp.path(), Strictness::Strict).unwrap();

        assert!(atoms.is_empty());
        insta::with_settings! {
            { description => wasmer_toml },
            { insta::assert_yaml_snapshot!(&transformed); }
        }
    }

    #[test]
    fn transform_manifest_with_single_atom() {
        let temp = TempDir::new().unwrap();
        let wasmer_toml = r#"
            [package]
            name = "some/package"
            version = "0.0.0"
            description = "My awesome package"

            [[module]]
            name = "first"
            source = "./path/to/file.wasm"
            abi = "wasi"
        "#;
        let manifest: WasmerManifest = toml::from_str(wasmer_toml).unwrap();
        let dir = temp.path().join("path").join("to");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("file.wasm"), b"\0asm...").unwrap();

        let (transformed, atoms) =
            wasmer_manifest_to_webc(&manifest, temp.path(), Strictness::Strict).unwrap();

        assert_eq!(atoms.len(), 1);
        assert_eq!(atoms["first"].as_slice(), b"\0asm...");
        insta::with_settings! {
            { description => wasmer_toml },
            { insta::assert_yaml_snapshot!(&transformed); }
        }
    }

    #[test]
    fn transform_manifest_with_atom_and_command() {
        let temp = TempDir::new().unwrap();
        let wasmer_toml = r#"
            [package]
            name = "some/package"
            version = "0.0.0"
            description = "My awesome package"

            [[module]]
            name = "cpython"
            source = "python.wasm"
            abi = "wasi"

            [[command]]
            name = "python"
            module = "cpython"
            runner = "wasi"
        "#;
        let manifest: WasmerManifest = toml::from_str(wasmer_toml).unwrap();
        std::fs::write(temp.path().join("python.wasm"), b"\0asm...").unwrap();

        let (transformed, _) =
            wasmer_manifest_to_webc(&manifest, temp.path(), Strictness::Strict).unwrap();

        assert_eq!(transformed.commands.len(), 1);
        let python = &transformed.commands["python"];
        assert_eq!(&python.runner, webc::metadata::annotations::WASI_RUNNER_URI);
        assert_eq!(python.wasi().unwrap().unwrap(), Wasi::new("cpython"));
        insta::with_settings! {
            { description => wasmer_toml },
            { insta::assert_yaml_snapshot!(&transformed); }
        }
    }

    #[test]
    fn transform_manifest_with_multiple_commands() {
        let temp = TempDir::new().unwrap();
        let wasmer_toml = r#"
            [package]
            name = "some/package"
            version = "0.0.0"
            description = "My awesome package"

            [[module]]
            name = "cpython"
            source = "python.wasm"
            abi = "wasi"

            [[command]]
            name = "first"
            module = "cpython"
            runner = "wasi"

            [[command]]
            name = "second"
            module = "cpython"
            runner = "wasi"

            [[command]]
            name = "third"
            module = "cpython"
            runner = "wasi"
        "#;
        let manifest: WasmerManifest = toml::from_str(wasmer_toml).unwrap();
        std::fs::write(temp.path().join("python.wasm"), b"\0asm...").unwrap();

        let (transformed, _) =
            wasmer_manifest_to_webc(&manifest, temp.path(), Strictness::Strict).unwrap();

        assert_eq!(transformed.commands.len(), 3);
        assert!(transformed.commands.contains_key("first"));
        assert!(transformed.commands.contains_key("second"));
        assert!(transformed.commands.contains_key("third"));
        insta::with_settings! {
            { description => wasmer_toml },
            { insta::assert_yaml_snapshot!(&transformed); }
        }
    }

    #[test]
    fn merge_custom_attributes_with_builtin_ones() {
        let temp = TempDir::new().unwrap();
        let wasmer_toml = r#"
            [package]
            name = "some/package"
            version = "0.0.0"
            description = "My awesome package"

            [[module]]
            name = "cpython"
            source = "python.wasm"
            abi = "wasi"

            [[command]]
            name = "python"
            module = "cpython"
            runner = "wasi"
            annotations = { wasi = { env = ["KEY=val"]} }
        "#;
        let manifest: WasmerManifest = toml::from_str(wasmer_toml).unwrap();
        std::fs::write(temp.path().join("python.wasm"), b"\0asm...").unwrap();

        let (transformed, _) =
            wasmer_manifest_to_webc(&manifest, temp.path(), Strictness::Strict).unwrap();

        assert_eq!(transformed.commands.len(), 1);
        let cmd = &transformed.commands["python"];
        assert_eq!(
            &cmd.wasi().unwrap().unwrap(),
            Wasi::new("cpython").with_env("KEY", "val")
        );
        insta::with_settings! {
            { description => wasmer_toml },
            { insta::assert_yaml_snapshot!(&transformed); }
        }
    }

    #[test]
    fn transform_bash_manifest() {
        let temp = TempDir::new().unwrap();
        let wasmer_toml = r#"
            [package]
            name = "sharrattj/bash"
            version = "1.0.17"
            description = "Bash is a modern POSIX-compliant implementation of /bin/sh."
            license = "GNU"
            wasmer-extra-flags = "--enable-threads --enable-bulk-memory"

            [dependencies]
            "sharrattj/coreutils" = "1.0.16"

            [[module]]
            name = "bash"
            source = "bash.wasm"
            abi = "wasi"

            [[command]]
            name = "bash"
            module = "bash"
            runner = "wasi@unstable_"
        "#;
        let manifest: WasmerManifest = toml::from_str(wasmer_toml).unwrap();
        std::fs::write(temp.path().join("bash.wasm"), b"\0asm...").unwrap();

        let (transformed, _) =
            wasmer_manifest_to_webc(&manifest, temp.path(), Strictness::Strict).unwrap();

        insta::with_settings! {
            { description => wasmer_toml },
            { insta::assert_yaml_snapshot!(&transformed); }
        }
    }

    #[test]
    fn transform_wasmer_pack_manifest() {
        let temp = TempDir::new().unwrap();
        let wasmer_toml = r#"
            [package]
            name = "wasmer/wasmer-pack"
            version = "0.7.0"
            description = "The WebAssembly interface to wasmer-pack."
            license = "MIT"
            readme = "README.md"
            repository = "https://github.com/wasmerio/wasmer-pack"
            homepage = "https://wasmer.io/"

            [[module]]
            name = "wasmer-pack-wasm"
            source = "wasmer_pack_wasm.wasm"

            [module.bindings]
            wai-version = "0.2.0"
            exports = "wasmer-pack.exports.wai"
            imports = []
        "#;
        let manifest: WasmerManifest = toml::from_str(wasmer_toml).unwrap();
        std::fs::write(temp.path().join("wasmer_pack_wasm.wasm"), b"\0asm...").unwrap();
        std::fs::write(temp.path().join("wasmer-pack.exports.wai"), b"").unwrap();
        std::fs::write(temp.path().join("README.md"), b"").unwrap();

        let (transformed, _) =
            wasmer_manifest_to_webc(&manifest, temp.path(), Strictness::Strict).unwrap();

        insta::with_settings! {
            { description => wasmer_toml },
            { insta::assert_yaml_snapshot!(&transformed); }
        }
    }

    #[test]
    fn transform_python_manifest() {
        let temp = TempDir::new().unwrap();
        let wasmer_toml = r#"
            [package]
            name = "python"
            version = "0.1.0"
            description = "Python is an interpreted, high-level, general-purpose programming language"
            license = "ISC"
            repository = "https://github.com/wapm-packages/python"

            [[module]]
            name = "python"
            source = "bin/python.wasm"
            abi = "wasi"

            [module.interfaces]
            wasi = "0.0.0-unstable"

            [[command]]
            name = "python"
            module = "python"

            [fs]
            lib = "lib"
        "#;
        let manifest: WasmerManifest = toml::from_str(wasmer_toml).unwrap();
        let bin = temp.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("python.wasm"), b"\0asm...").unwrap();

        let (transformed, _) =
            wasmer_manifest_to_webc(&manifest, temp.path(), Strictness::Strict).unwrap();

        insta::with_settings! {
            { description => wasmer_toml },
            { insta::assert_yaml_snapshot!(&transformed); }
        }
    }

    #[test]
    fn transform_manifest_with_fs_table() {
        let temp = TempDir::new().unwrap();
        let wasmer_toml = r#"
            [package]
            name = "some/package"
            version = "0.0.0"
            description = "This is a package"

            [fs]
            lib = "lib"
            "/public" = "out"
        "#;
        let manifest: WasmerManifest = toml::from_str(wasmer_toml).unwrap();
        std::fs::write(temp.path().join("python.wasm"), b"\0asm...").unwrap();

        let (transformed, _) =
            wasmer_manifest_to_webc(&manifest, temp.path(), Strictness::Strict).unwrap();

        let fs = transformed.filesystem().unwrap().unwrap();
        assert_eq!(
            fs,
            [
                FileSystemMapping {
                    from: None,
                    volume_name: "/lib".to_string(),
                    host_path: None,
                    mount_path: "/lib".to_string(),
                },
                FileSystemMapping {
                    from: None,
                    volume_name: "/out".to_string(),
                    host_path: None,
                    mount_path: "/public".to_string(),
                }
            ]
        );
        insta::with_settings! {
            { description => wasmer_toml },
            { insta::assert_yaml_snapshot!(&transformed); }
        }
    }

    #[test]
    fn missing_command_dependency() {
        let temp = TempDir::new().unwrap();
        let wasmer_toml = r#"
            [[command]]
            name = "python"
            module = "test/python:python"
        "#;
        let manifest: WasmerManifest = toml::from_str(wasmer_toml).unwrap();
        let bin = temp.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let res = wasmer_manifest_to_webc(&manifest, temp.path(), Strictness::Strict);

        assert!(matches!(
            res,
            Err(ManifestError::UndeclaredCommandDependency { .. })
        ));
    }

    #[test]
    fn issue_124_command_runner_is_swallowed() {
        let temp = TempDir::new().unwrap();
        let wasmer_toml = r#"
            [package]
            name = "wasmer-tests/wcgi-always-panic"
            version = "0.1.0"
            description = "wasmer-tests/wcgi-always-panic website"

            [[module]]
            name = "wcgi-always-panic"
            source = "./wcgi-always-panic.wasm"
            abi = "wasi"

            [[command]]
            name = "wcgi"
            module = "wcgi-always-panic"
            runner = "https://webc.org/runner/wcgi"
        "#;
        let manifest: WasmerManifest = toml::from_str(wasmer_toml).unwrap();
        std::fs::write(temp.path().join("wcgi-always-panic.wasm"), b"\0asm...").unwrap();

        let (transformed, _) =
            wasmer_manifest_to_webc(&manifest, temp.path(), Strictness::Strict).unwrap();

        let cmd = &transformed.commands["wcgi"];
        assert_eq!(cmd.runner, webc::metadata::annotations::WCGI_RUNNER_URI);
        assert_eq!(cmd.wasi().unwrap().unwrap(), Wasi::new("wcgi-always-panic"));
        insta::with_settings! {
            { description => wasmer_toml },
            { insta::assert_yaml_snapshot!(&transformed); }
        }
    }
}
