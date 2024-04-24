//! Create a standalone native executable for a given Wasm file.

use std::{
    collections::BTreeMap,
    env,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{anyhow, bail, Context, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};
use tar::Archive;
use wasmer::sys::Artifact;
use wasmer::*;
use wasmer_object::{emit_serialized, get_object_for_target};
use wasmer_types::{compilation::symbols::ModuleMetadataSymbolRegistry, ModuleInfo};
use webc::{
    compat::{Container, Volume as WebcVolume},
    PathSegments,
};

use self::utils::normalize_atom_name;
use crate::{common::normalize_path, store::CompilerOptions};

const LINK_SYSTEM_LIBRARIES_WINDOWS: &[&str] = &["userenv", "Ws2_32", "advapi32", "bcrypt"];

const LINK_SYSTEM_LIBRARIES_UNIX: &[&str] = &["dl", "m", "pthread"];

#[derive(Debug, Parser)]
/// The options for the `wasmer create-exe` subcommand
pub struct CreateExe {
    /// Input file
    #[clap(name = "FILE")]
    path: PathBuf,

    /// Output file
    #[clap(name = "OUTPUT PATH", short = 'o')]
    output: PathBuf,

    /// Optional directorey used for debugging: if present, will output the zig command
    /// for reproducing issues in a debug directory
    #[clap(long, name = "DEBUG PATH")]
    debug_dir: Option<PathBuf>,

    /// Prefix for every input file, e.g. "wat2wasm:sha256abc123" would
    /// prefix every function in the wat2wasm input object with the "sha256abc123" hash
    ///
    /// If only a single value is given without containing a ":", this value is used for
    /// all input files. If no value is given, the prefix is always equal to
    /// the sha256 of the input .wasm file
    #[clap(
        long,
        use_value_delimiter = true,
        value_delimiter = ',',
        name = "FILE:PREFIX:PATH"
    )]
    precompiled_atom: Vec<String>,

    /// Compilation Target triple
    ///
    /// Accepted target triple values must follow the
    /// ['target_lexicon'](https://crates.io/crates/target-lexicon) crate format.
    ///
    /// The recommended targets we try to support are:
    ///
    /// - "x86_64-linux-gnu"
    /// - "aarch64-linux-gnu"
    /// - "x86_64-apple-darwin"
    /// - "arm64-apple-darwin"
    /// - "x86_64-windows-gnu"
    #[clap(long = "target")]
    target_triple: Option<Triple>,

    /// Can specify either a release version (such as "3.0.1") or a URL to a tarball to use
    /// for linking. By default, create-exe will always pull the latest release tarball from GitHub,
    /// this flag can be used to override that behaviour.
    #[clap(long, name = "URL_OR_RELEASE_VERSION")]
    use_wasmer_release: Option<String>,

    #[clap(long, short = 'm', number_of_values = 1)]
    cpu_features: Vec<CpuFeature>,

    /// Additional libraries to link against.
    /// This is useful for fixing linker errors that may occur on some systems.
    #[clap(long, short = 'l')]
    libraries: Vec<String>,

    #[clap(flatten)]
    cross_compile: CrossCompile,

    #[clap(flatten)]
    compiler: CompilerOptions,
}

/// Url or version to download the release from
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UrlOrVersion {
    /// URL to download
    Url(url::Url),
    /// Release version to download
    Version(semver::Version),
}

impl UrlOrVersion {
    fn from_str(s: &str) -> Result<Self, anyhow::Error> {
        let mut err;
        let s = s.strip_prefix('v').unwrap_or(s);
        match url::Url::parse(s) {
            Ok(o) => return Ok(Self::Url(o)),
            Err(e) => {
                err = anyhow::anyhow!("could not parse as URL: {e}");
            }
        }

        match semver::Version::parse(s) {
            Ok(o) => return Ok(Self::Version(o)),
            Err(e) => {
                err = anyhow::anyhow!("could not parse as URL or version: {e}").context(err);
            }
        }

        Err(err)
    }
}

// Cross-compilation options with `zig`
#[derive(Debug, Clone, Default, Parser)]
pub(crate) struct CrossCompile {
    /// Use the system linker instead of zig for linking
    #[clap(long)]
    use_system_linker: bool,

    /// Cross-compilation library path (path to libwasmer.a / wasmer.lib)
    #[clap(long = "library-path")]
    library_path: Option<PathBuf>,

    /// Cross-compilation tarball library path
    #[clap(long = "tarball")]
    tarball: Option<PathBuf>,

    /// Specify `zig` binary path (defaults to `zig` in $PATH if not present)
    #[clap(long = "zig-binary-path", env)]
    zig_binary_path: Option<PathBuf>,
}

#[derive(Debug)]
pub(crate) struct CrossCompileSetup {
    pub(crate) target: Triple,
    pub(crate) zig_binary_path: Option<PathBuf>,
    pub(crate) library: PathBuf,
}

/// Given a pirita file, determines whether the file has one
/// default command as an entrypoint or multiple (need to be specified via --command)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entrypoint {
    /// Compiled atom files to link into the final binary
    pub atoms: Vec<CommandEntrypoint>,
    /// Volume objects (if any) to link into the final binary
    pub volumes: Vec<Volume>,
}

/// Command entrypoint for multiple commands
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandEntrypoint {
    /// Command name
    pub command: String,
    /// Atom name
    pub atom: String,
    /// Path to the object file, relative to the entrypoint.json parent dir
    pub path: PathBuf,
    /// Optional path to the static_defs.h header file, relative to the entrypoint.json parent dir
    pub header: Option<PathBuf>,
    /// Module info, set when the wasm file is compiled
    pub module_info: Option<ModuleInfo>,
}

/// Volume object file (name + path to object file)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Volume {
    /// Volume name
    pub name: String,
    /// Path to volume fileblock object file
    pub obj_file: PathBuf,
}

impl CreateExe {
    /// Runs logic for the `compile` subcommand
    pub fn execute(&self) -> Result<()> {
        let path = normalize_path(&format!("{}", self.path.display()));
        let target_triple = self.target_triple.clone().unwrap_or_else(Triple::host);
        let mut cc = self.cross_compile.clone();
        let target = utils::target_triple_to_target(&target_triple, &self.cpu_features);

        let starting_cd = env::current_dir()?;
        let input_path = starting_cd.join(path);
        let output_path = starting_cd.join(&self.output);

        let url_or_version = match self
            .use_wasmer_release
            .as_deref()
            .map(UrlOrVersion::from_str)
        {
            Some(Ok(o)) => Some(o),
            Some(Err(e)) => return Err(e),
            None => None,
        };

        let cross_compilation =
            utils::get_cross_compile_setup(&mut cc, &target_triple, &starting_cd, url_or_version)?;

        if input_path.is_dir() {
            return Err(anyhow::anyhow!("input path cannot be a directory"));
        }

        let (store, compiler_type) = self.compiler.get_store_for_target(target.clone())?;

        println!("Compiler: {}", compiler_type.to_string());
        println!("Target: {}", target.triple());
        println!(
            "Using path `{}` as libwasmer path.",
            cross_compilation.library.display()
        );

        if !cross_compilation.library.exists() {
            return Err(anyhow::anyhow!("library path does not exist"));
        }

        let temp = tempfile::tempdir();
        let tempdir = match self.debug_dir.as_ref() {
            Some(s) => s.clone(),
            None => temp?.path().to_path_buf(),
        };
        std::fs::create_dir_all(&tempdir)?;

        let atoms = if let Ok(pirita) = Container::from_disk(&input_path) {
            // pirita file
            compile_pirita_into_directory(
                &pirita,
                &tempdir,
                &self.compiler,
                &self.cpu_features,
                &cross_compilation.target,
                &self.precompiled_atom,
                AllowMultiWasm::Allow,
                self.debug_dir.is_some(),
            )
        } else {
            // wasm file
            prepare_directory_from_single_wasm_file(
                &input_path,
                &tempdir,
                &self.compiler,
                &cross_compilation.target,
                &self.cpu_features,
                &self.precompiled_atom,
                self.debug_dir.is_some(),
            )
        }?;

        get_module_infos(&store, &tempdir, &atoms)?;
        let mut entrypoint = get_entrypoint(&tempdir)?;
        create_header_files_in_dir(&tempdir, &mut entrypoint, &atoms, &self.precompiled_atom)?;
        link_exe_from_dir(
            &tempdir,
            output_path,
            &cross_compilation,
            &self.libraries,
            self.debug_dir.is_some(),
            &atoms,
            &self.precompiled_atom,
        )?;

        if self.target_triple.is_some() {
            eprintln!(
                "✔ Cross-compiled executable for `{}` target compiled successfully to `{}`.",
                target.triple(),
                self.output.display(),
            );
        } else {
            eprintln!(
                "✔ Native executable compiled successfully to `{}`.",
                self.output.display(),
            );
        }

        Ok(())
    }
}

fn write_entrypoint(directory: &Path, entrypoint: &Entrypoint) -> Result<(), anyhow::Error> {
    std::fs::write(
        directory.join("entrypoint.json"),
        serde_json::to_string_pretty(&entrypoint).unwrap(),
    )
    .map_err(|e| {
        anyhow::anyhow!(
            "cannot create entrypoint.json dir in {}: {e}",
            directory.display()
        )
    })
}

fn get_entrypoint(directory: &Path) -> Result<Entrypoint, anyhow::Error> {
    let entrypoint_json =
        std::fs::read_to_string(directory.join("entrypoint.json")).map_err(|e| {
            anyhow::anyhow!(
                "could not read entrypoint.json in {}: {e}",
                directory.display()
            )
        })?;

    let entrypoint: Entrypoint = serde_json::from_str(&entrypoint_json).map_err(|e| {
        anyhow::anyhow!(
            "could not parse entrypoint.json in {}: {e}",
            directory.display()
        )
    })?;

    if entrypoint.atoms.is_empty() {
        return Err(anyhow::anyhow!("file has no atoms to compile"));
    }

    Ok(entrypoint)
}

/// In pirita mode, specifies whether multi-atom
/// pirita files should be allowed or rejected
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum AllowMultiWasm {
    /// allow
    Allow,
    /// reject
    Reject(Option<String>),
}

/// Given a pirita file, compiles the .wasm files into the target directory
#[allow(clippy::too_many_arguments)]
pub(super) fn compile_pirita_into_directory(
    pirita: &Container,
    target_dir: &Path,
    compiler: &CompilerOptions,
    cpu_features: &[CpuFeature],
    triple: &Triple,
    prefixes: &[String],
    allow_multi_wasm: AllowMultiWasm,
    debug: bool,
) -> anyhow::Result<Vec<(String, Vec<u8>)>> {
    let all_atoms = match &allow_multi_wasm {
        AllowMultiWasm::Allow | AllowMultiWasm::Reject(None) => {
            pirita.atoms().into_iter().collect::<Vec<_>>()
        }
        AllowMultiWasm::Reject(Some(s)) => {
            let atom = pirita
                .get_atom(s)
                .with_context(|| format!("could not find atom \"{s}\"",))?;
            vec![(s.to_string(), atom)]
        }
    };

    allow_multi_wasm.validate(&all_atoms)?;

    std::fs::create_dir_all(target_dir)
        .map_err(|e| anyhow::anyhow!("cannot create / dir in {}: {e}", target_dir.display()))?;

    let target_dir = target_dir.canonicalize()?;
    let target = &utils::target_triple_to_target(triple, cpu_features);

    std::fs::create_dir_all(target_dir.join("volumes")).map_err(|e| {
        anyhow::anyhow!(
            "cannot create /volumes dir in {}: {e}",
            target_dir.display()
        )
    })?;

    let volumes = pirita.volumes();
    let volume_bytes = volume_file_block(&volumes);
    let volume_name = "VOLUMES";
    let volume_path = target_dir.join("volumes").join("volume.o");
    write_volume_obj(&volume_bytes, volume_name, &volume_path, target)?;
    let volume_path = volume_path.canonicalize()?;
    let volume_path = pathdiff::diff_paths(&volume_path, &target_dir).unwrap();

    std::fs::create_dir_all(target_dir.join("atoms")).map_err(|e| {
        anyhow::anyhow!("cannot create /atoms dir in {}: {e}", target_dir.display())
    })?;

    let mut atoms_from_file = Vec::new();
    let mut target_paths = Vec::new();

    for (atom_name, atom_bytes) in all_atoms {
        atoms_from_file.push((utils::normalize_atom_name(&atom_name), atom_bytes.to_vec()));
        let atom_path = target_dir
            .join("atoms")
            .join(format!("{}.o", utils::normalize_atom_name(&atom_name)));
        let header_path = {
            std::fs::create_dir_all(target_dir.join("include")).map_err(|e| {
                anyhow::anyhow!(
                    "cannot create /include dir in {}: {e}",
                    target_dir.display()
                )
            })?;

            let header_path = target_dir.join("include").join(format!(
                "static_defs_{}.h",
                utils::normalize_atom_name(&atom_name)
            ));

            Some(header_path)
        };
        target_paths.push((
            atom_name.clone(),
            utils::normalize_atom_name(&atom_name),
            atom_path,
            header_path,
        ));
    }

    let prefix_map = PrefixMapCompilation::from_input(&atoms_from_file, prefixes, false)
        .with_context(|| anyhow::anyhow!("compile_pirita_into_directory"))?;

    let module_infos = compile_atoms(
        &atoms_from_file,
        &target_dir.join("atoms"),
        compiler,
        target,
        &prefix_map,
        debug,
    )?;

    // target_dir
    let mut atoms = Vec::new();
    for (command_name, atom_name, a, opt_header_path) in target_paths {
        let mut atom_path = a;
        let mut header_path = opt_header_path;
        if let Ok(a) = atom_path.canonicalize() {
            let opt_header_path = header_path.and_then(|p| p.canonicalize().ok());
            atom_path = pathdiff::diff_paths(&a, &target_dir).unwrap_or_else(|| a.clone());
            header_path = opt_header_path.and_then(|h| pathdiff::diff_paths(h, &target_dir));
        }
        atoms.push(CommandEntrypoint {
            // TODO: improve, "--command pip" should be able to invoke atom "python" with args "-m pip"
            command: command_name,
            atom: atom_name.clone(),
            path: atom_path,
            header: header_path,
            module_info: module_infos.get(&atom_name).cloned(),
        });
    }

    let entrypoint = Entrypoint {
        atoms,
        volumes: vec![Volume {
            name: volume_name.to_string(),
            obj_file: volume_path,
        }],
    };

    write_entrypoint(&target_dir, &entrypoint)?;

    Ok(atoms_from_file)
}

/// Serialize a set of volumes so they can be read by the C API.
///
/// This is really backwards, but the only way to create a v1 volume "fileblock"
/// is by first reading every file in a [`webc::compat::Volume`], serializing it
/// to webc v1's binary form, parsing those bytes into a [`webc::v1::Volume`],
/// then constructing a dummy [`webc::v1::WebC`] object just so we can make a
/// copy of its file block.
fn volume_file_block(volumes: &BTreeMap<String, WebcVolume>) -> Vec<u8> {
    let serialized_volumes: Vec<(&String, Vec<u8>)> = volumes
        .iter()
        .map(|(name, volume)| (name, serialize_volume_to_webc_v1(volume)))
        .collect();

    let parsed_volumes: indexmap::IndexMap<String, webc::v1::Volume<'_>> = serialized_volumes
        .iter()
        .filter_map(|(name, serialized_volume)| {
            let volume = webc::v1::Volume::parse(serialized_volume).ok()?;
            Some((name.to_string(), volume))
        })
        .collect();

    let webc = webc::v1::WebC {
        version: 0,
        checksum: None,
        signature: None,
        manifest: webc::metadata::Manifest::default(),
        atoms: webc::v1::Volume::default(),
        volumes: parsed_volumes,
    };

    webc.get_volumes_as_fileblock()
}

fn serialize_volume_to_webc_v1(volume: &WebcVolume) -> Vec<u8> {
    fn read_dir(
        volume: &WebcVolume,
        path: &mut PathSegments,
        files: &mut BTreeMap<webc::v1::DirOrFile, Vec<u8>>,
    ) {
        for (segment, _, meta) in volume.read_dir(&*path).unwrap_or_default() {
            path.push(segment);

            match meta {
                webc::compat::Metadata::Dir { .. } => {
                    files.insert(
                        webc::v1::DirOrFile::Dir(path.to_string().into()),
                        Vec::new(),
                    );
                    read_dir(volume, path, files);
                }
                webc::compat::Metadata::File { .. } => {
                    if let Some((contents, _)) = volume.read_file(&*path) {
                        files.insert(
                            webc::v1::DirOrFile::File(path.to_string().into()),
                            contents.to_vec(),
                        );
                    }
                }
            }

            path.pop();
        }
    }

    let mut path = PathSegments::ROOT;
    let mut files = BTreeMap::new();

    read_dir(volume, &mut path, &mut files);

    webc::v1::Volume::serialize_files(files)
}

impl AllowMultiWasm {
    fn validate(
        &self,
        all_atoms: &Vec<(String, webc::compat::SharedBytes)>,
    ) -> Result<(), anyhow::Error> {
        if matches!(self, AllowMultiWasm::Reject(None)) && all_atoms.len() > 1 {
            let keys = all_atoms
                .iter()
                .map(|(name, _)| name.clone())
                .collect::<Vec<_>>();

            return Err(anyhow::anyhow!(
                "where <ATOM> is one of: {}",
                keys.join(", ")
            ))
            .context(anyhow::anyhow!(
                "note: use --atom <ATOM> to specify which atom to compile"
            ))
            .context(anyhow::anyhow!(
                "cannot compile more than one atom at a time"
            ));
        }

        Ok(())
    }
}

/// Prefix map used during compilation of object files
#[derive(Debug, Default, PartialEq)]
pub(crate) struct PrefixMapCompilation {
    /// Sha256 hashes for the input files
    input_hashes: BTreeMap<String, String>,
    /// Manual prefixes for input files (file:prefix)
    manual_prefixes: BTreeMap<String, String>,
    /// Cached compilation objects for files on disk
    #[allow(dead_code)]
    compilation_objects: BTreeMap<String, Vec<u8>>,
}

impl PrefixMapCompilation {
    /// Sets up the prefix map from a collection like "sha123123" or "wasmfile:sha123123" or "wasmfile:/tmp/filepath/:sha123123"
    fn from_input(
        atoms: &[(String, Vec<u8>)],
        prefixes: &[String],
        only_validate_prefixes: bool,
    ) -> Result<Self, anyhow::Error> {
        // No atoms: no prefixes necessary
        if atoms.is_empty() {
            return Ok(Self::default());
        }

        // default case: no prefixes have been specified:
        // for all atoms, calculate the sha256
        if prefixes.is_empty() {
            return Ok(Self {
                input_hashes: atoms
                    .iter()
                    .map(|(name, bytes)| (normalize_atom_name(name), Self::hash_for_bytes(bytes)))
                    .collect(),
                manual_prefixes: BTreeMap::new(),
                compilation_objects: BTreeMap::new(),
            });
        }

        // if prefixes are specified, have to match the atom names exactly
        if prefixes.len() != atoms.len() {
            println!(
                "WARNING: invalid mapping of prefix and atoms: expected prefixes for {} atoms, got {} prefixes",
                atoms.len(), prefixes.len()
            );
        }

        let available_atoms = atoms.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>();
        let mut manual_prefixes = BTreeMap::new();
        let mut compilation_objects = BTreeMap::new();

        for p in prefixes.iter() {
            // On windows, we have to account for paths like "C://", which would
            // prevent a simple .split(":") or a regex
            let prefix_split = Self::split_prefix(p);

            match prefix_split.as_slice() {
                // ATOM:PREFIX:PATH
                [atom, prefix, path] => {
                    if only_validate_prefixes {
                        // only insert the prefix in order to not error out of the fs::read(path)
                        manual_prefixes.insert(normalize_atom_name(atom), prefix.to_string());
                    } else {
                        let atom_hash = atoms
                        .iter()
                        .find_map(|(name, _)| if normalize_atom_name(name) == normalize_atom_name(atom) { Some(prefix.to_string()) } else { None })
                        .ok_or_else(|| anyhow::anyhow!("no atom {atom:?} found, for prefix {p:?}, available atoms are {available_atoms:?}"))?;

                        let current_dir = std::env::current_dir().unwrap().canonicalize().unwrap();
                        let path = current_dir.join(path.replace("./", ""));
                        let bytes = std::fs::read(&path).map_err(|e| {
                            anyhow::anyhow!("could not read file for atom {atom:?} (prefix {p}, path {} in dir {}): {e}", path.display(), current_dir.display())
                        })?;

                        compilation_objects.insert(normalize_atom_name(atom), bytes);
                        manual_prefixes.insert(normalize_atom_name(atom), atom_hash.to_string());
                    }
                }
                // atom + path, but default SHA256 prefix
                [atom, path] => {
                    let atom_hash = atoms
                    .iter()
                    .find_map(|(name, bytes)| if normalize_atom_name(name) == normalize_atom_name(atom) { Some(Self::hash_for_bytes(bytes)) } else { None })
                    .ok_or_else(|| anyhow::anyhow!("no atom {atom:?} found, for prefix {p:?}, available atoms are {available_atoms:?}"))?;
                    manual_prefixes.insert(normalize_atom_name(atom), atom_hash.to_string());

                    if !only_validate_prefixes {
                        let current_dir = std::env::current_dir().unwrap().canonicalize().unwrap();
                        let path = current_dir.join(path.replace("./", ""));
                        let bytes = std::fs::read(&path).map_err(|e| {
                            anyhow::anyhow!("could not read file for atom {atom:?} (prefix {p}, path {} in dir {}): {e}", path.display(), current_dir.display())
                        })?;
                        compilation_objects.insert(normalize_atom_name(atom), bytes);
                    }
                }
                // only prefix if atoms.len() == 1
                [prefix] if atoms.len() == 1 => {
                    manual_prefixes.insert(normalize_atom_name(&atoms[0].0), prefix.to_string());
                }
                _ => {
                    return Err(anyhow::anyhow!("invalid --precompiled-atom {p:?} - correct format is ATOM:PREFIX:PATH or ATOM:PATH"));
                }
            }
        }

        Ok(Self {
            input_hashes: BTreeMap::new(),
            manual_prefixes,
            compilation_objects,
        })
    }

    fn split_prefix(s: &str) -> Vec<String> {
        let regex =
            regex::Regex::new(r"^([a-zA-Z0-9\-_]+)(:([a-zA-Z0-9\.\-_]+))?(:(.+*))?").unwrap();
        let mut captures = regex
            .captures(s.trim())
            .map(|c| {
                c.iter()
                    .skip(1)
                    .flatten()
                    .map(|m| m.as_str().to_owned())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if captures.is_empty() {
            vec![s.to_string()]
        } else if captures.len() == 5 {
            captures.remove(1);
            captures.remove(2);
            captures
        } else if captures.len() == 3 {
            s.splitn(2, ':').map(|s| s.to_string()).collect()
        } else {
            captures
        }
    }

    pub(crate) fn hash_for_bytes(bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let result = hasher.finalize();

        hex::encode(&result[..])
    }

    fn get_prefix_for_atom(&self, atom_name: &str) -> Option<String> {
        self.manual_prefixes
            .get(atom_name)
            .or_else(|| self.input_hashes.get(atom_name))
            .cloned()
    }

    #[allow(dead_code)]
    fn get_compilation_object_for_atom(&self, atom_name: &str) -> Option<&[u8]> {
        self.compilation_objects
            .get(atom_name)
            .map(|s| s.as_slice())
    }
}

#[test]
fn test_prefix_parsing() {
    let tempdir = tempfile::TempDir::new().unwrap();
    let path = tempdir.path();
    std::fs::write(path.join("test.obj"), b"").unwrap();
    let str1 = format!("ATOM_NAME:PREFIX:{}", path.join("test.obj").display());
    let prefix =
        PrefixMapCompilation::from_input(&[("ATOM_NAME".to_string(), b"".to_vec())], &[str1], true);
    assert_eq!(
        prefix.unwrap(),
        PrefixMapCompilation {
            input_hashes: BTreeMap::new(),
            manual_prefixes: vec![("ATOM_NAME".to_string(), "PREFIX".to_string())]
                .into_iter()
                .collect(),
            compilation_objects: Vec::new().into_iter().collect(),
        }
    );
}

#[test]
fn test_split_prefix() {
    let split = PrefixMapCompilation::split_prefix(
        "qjs:abc123:C:\\Users\\felix\\AppData\\Local\\Temp\\.tmpoccCjV\\wasm.obj",
    );
    assert_eq!(
        split,
        vec![
            "qjs".to_string(),
            "abc123".to_string(),
            "C:\\Users\\felix\\AppData\\Local\\Temp\\.tmpoccCjV\\wasm.obj".to_string(),
        ]
    );
    let split = PrefixMapCompilation::split_prefix("qjs:./tmp.obj");
    assert_eq!(split, vec!["qjs".to_string(), "./tmp.obj".to_string(),]);
    let split2 = PrefixMapCompilation::split_prefix(
        "qjs:abc123:/var/folders/65/2zzy98b16xz254jccxjzqb8w0000gn/T/.tmpNdgVaq/wasm.o",
    );
    assert_eq!(
        split2,
        vec![
            "qjs".to_string(),
            "abc123".to_string(),
            "/var/folders/65/2zzy98b16xz254jccxjzqb8w0000gn/T/.tmpNdgVaq/wasm.o".to_string(),
        ]
    );
    let split3 = PrefixMapCompilation::split_prefix(
        "qjs:/var/folders/65/2zzy98b16xz254jccxjzqb8w0000gn/T/.tmpNdgVaq/wasm.o",
    );
    assert_eq!(
        split3,
        vec![
            "qjs".to_string(),
            "/var/folders/65/2zzy98b16xz254jccxjzqb8w0000gn/T/.tmpNdgVaq/wasm.o".to_string(),
        ]
    );
}

fn compile_atoms(
    atoms: &[(String, Vec<u8>)],
    output_dir: &Path,
    compiler: &CompilerOptions,
    target: &Target,
    prefixes: &PrefixMapCompilation,
    debug: bool,
) -> Result<BTreeMap<String, ModuleInfo>, anyhow::Error> {
    use std::{
        fs::File,
        io::{BufWriter, Write},
    };

    let mut module_infos = BTreeMap::new();
    for (a, data) in atoms {
        let prefix = prefixes
            .get_prefix_for_atom(&normalize_atom_name(a))
            .ok_or_else(|| anyhow::anyhow!("no prefix given for atom {a}"))?;
        let atom_name = utils::normalize_atom_name(a);
        let output_object_path = output_dir.join(format!("{atom_name}.o"));
        if let Some(atom) = prefixes.get_compilation_object_for_atom(a) {
            std::fs::write(&output_object_path, atom)
                .map_err(|e| anyhow::anyhow!("{}: {e}", output_object_path.display()))?;
            if debug {
                println!("Using cached object file for atom {a:?}.");
            }
            continue;
        }
        let (engine, _) = compiler.get_engine_for_target(target.clone())?;
        let engine_inner = engine.inner();
        let compiler = engine_inner.compiler()?;
        let features = engine_inner.features();
        let tunables = engine.tunables();
        let (module_info, obj, _, _) = Artifact::generate_object(
            compiler,
            data,
            Some(prefix.as_str()),
            target,
            tunables,
            features,
        )?;
        module_infos.insert(atom_name, module_info);
        // Write object file with functions
        let mut writer = BufWriter::new(File::create(&output_object_path)?);
        obj.write_stream(&mut writer)
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;
        writer.flush()?;
    }

    Ok(module_infos)
}

/// Compile the C code.
fn run_c_compile(
    path_to_c_src: &Path,
    output_name: &Path,
    target: &Triple,
    debug: bool,
    include_dirs: &[PathBuf],
) -> anyhow::Result<()> {
    #[cfg(not(windows))]
    let c_compiler = "cc";
    // We must use a C++ compiler on Windows because wasm.h uses `static_assert`
    // which isn't available in `clang` on Windows.
    #[cfg(windows)]
    let c_compiler = "clang++";

    let mut command = Command::new(c_compiler);
    let mut command = command
        .arg("-Wall")
        .arg("-O2")
        .arg("-c")
        .arg(path_to_c_src)
        .arg("-I")
        .arg(utils::get_wasmer_include_directory()?);

    for i in include_dirs {
        command = command.arg("-I");
        command = command.arg(normalize_path(&i.display().to_string()));
    }

    // On some compiler -target isn't implemented
    if *target != Triple::host() {
        command = command.arg("-target").arg(format!("{}", target));
    }

    let command = command.arg("-o").arg(output_name);

    if debug {
        println!("{command:#?}");
    }

    let output = command.output()?;
    if debug {
        eprintln!(
            "run_c_compile: stdout: {}\n\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    if !output.status.success() {
        bail!(
            "C code compile failed with: stdout: {}\n\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(())
}

fn write_volume_obj(
    volume_bytes: &[u8],
    object_name: &str,
    output_path: &Path,
    target: &Target,
) -> anyhow::Result<()> {
    use std::{
        fs::File,
        io::{BufWriter, Write},
    };

    let mut volumes_object = get_object_for_target(target.triple())?;
    emit_serialized(
        &mut volumes_object,
        volume_bytes,
        target.triple(),
        object_name,
    )?;

    let mut writer = BufWriter::new(File::create(output_path)?);
    volumes_object
        .write_stream(&mut writer)
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    writer.flush()?;
    drop(writer);

    Ok(())
}

/// Given a .wasm file, compiles the .wasm file into the target directory and creates the entrypoint.json
#[allow(clippy::too_many_arguments)]
pub(super) fn prepare_directory_from_single_wasm_file(
    wasm_file: &Path,
    target_dir: &Path,
    compiler: &CompilerOptions,
    triple: &Triple,
    cpu_features: &[CpuFeature],
    prefix: &[String],
    debug: bool,
) -> anyhow::Result<Vec<(String, Vec<u8>)>, anyhow::Error> {
    let bytes = std::fs::read(wasm_file)?;
    let target = &utils::target_triple_to_target(triple, cpu_features);

    std::fs::create_dir_all(target_dir)
        .map_err(|e| anyhow::anyhow!("cannot create / dir in {}: {e}", target_dir.display()))?;

    let target_dir = target_dir.canonicalize()?;

    std::fs::create_dir_all(target_dir.join("atoms")).map_err(|e| {
        anyhow::anyhow!("cannot create /atoms dir in {}: {e}", target_dir.display())
    })?;

    let mut atoms_from_file = Vec::new();
    let mut target_paths = Vec::new();

    let all_files = vec![(
        wasm_file
            .file_stem()
            .and_then(|f| f.to_str())
            .unwrap_or("main")
            .to_string(),
        bytes,
    )];

    for (atom_name, atom_bytes) in all_files.iter() {
        atoms_from_file.push((atom_name.clone(), atom_bytes.to_vec()));
        let atom_path = target_dir.join("atoms").join(format!("{atom_name}.o"));
        target_paths.push((atom_name, atom_path));
    }

    let prefix_map = PrefixMapCompilation::from_input(&atoms_from_file, prefix, false)
        .with_context(|| anyhow::anyhow!("prepare_directory_from_single_wasm_file"))?;

    let module_infos = compile_atoms(
        &atoms_from_file,
        &target_dir.join("atoms"),
        compiler,
        target,
        &prefix_map,
        debug,
    )?;

    let mut atoms = Vec::new();
    for (atom_name, atom_path) in target_paths {
        atoms.push(CommandEntrypoint {
            // TODO: improve, "--command pip" should be able to invoke atom "python" with args "-m pip"
            command: atom_name.clone(),
            atom: atom_name.clone(),
            path: atom_path,
            header: None,
            module_info: module_infos.get(atom_name).cloned(),
        });
    }

    let entrypoint = Entrypoint {
        atoms,
        volumes: Vec::new(),
    };

    write_entrypoint(&target_dir, &entrypoint)?;

    Ok(all_files)
}

// Given the input file paths, correctly resolves the .wasm files,
// reads the module info from the wasm module and writes the ModuleInfo for each file
// into the entrypoint.json file
fn get_module_infos(
    store: &Store,
    directory: &Path,
    atoms: &[(String, Vec<u8>)],
) -> Result<BTreeMap<String, ModuleInfo>, anyhow::Error> {
    let mut entrypoint =
        get_entrypoint(directory).with_context(|| anyhow::anyhow!("get module infos"))?;

    let mut module_infos = BTreeMap::new();
    for (atom_name, atom_bytes) in atoms {
        let module = Module::new(&store, atom_bytes.as_slice())?;
        let module_info = module.info();
        if let Some(s) = entrypoint
            .atoms
            .iter_mut()
            .find(|a| a.atom.as_str() == atom_name.as_str())
        {
            s.module_info = Some(module_info.clone());
            module_infos.insert(atom_name.clone(), module_info.clone());
        }
    }

    write_entrypoint(directory, &entrypoint)?;

    Ok(module_infos)
}

/// Create the static_defs.h header files in the /include directory
pub(crate) fn create_header_files_in_dir(
    directory: &Path,
    entrypoint: &mut Entrypoint,
    atoms: &[(String, Vec<u8>)],
    prefixes: &[String],
) -> anyhow::Result<()> {
    use object::{Object, ObjectSection};

    std::fs::create_dir_all(directory.join("include")).map_err(|e| {
        anyhow::anyhow!("cannot create /include dir in {}: {e}", directory.display())
    })?;

    let prefixes = PrefixMapCompilation::from_input(atoms, prefixes, false)
        .with_context(|| anyhow::anyhow!("create_header_files_in_dir"))?;

    for atom in entrypoint.atoms.iter_mut() {
        let atom_name = &atom.atom;
        let prefix = prefixes
            .get_prefix_for_atom(atom_name)
            .ok_or_else(|| anyhow::anyhow!("cannot get prefix for atom {atom_name}"))?;

        let object_file_src = directory.join(&atom.path);
        let object_file = std::fs::read(&object_file_src)
            .map_err(|e| anyhow::anyhow!("could not read {}: {e}", object_file_src.display()))?;
        let obj_file = object::File::parse(&*object_file)?;
        let sections = obj_file
            .sections()
            .filter_map(|s| s.name().ok().map(|s| s.to_string()))
            .collect::<Vec<_>>();
        let section = obj_file
            .section_by_name(".data")
            .unwrap()
            .data()
            .map_err(|_| {
                anyhow::anyhow!(
                    "missing section .data in object file {} (sections = {:#?}",
                    object_file_src.display(),
                    sections
                )
            })?;
        let metadata_length = section.len();

        let module_info = atom
            .module_info
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no module info for atom {atom_name:?}"))?;

        let base_path = Path::new("include").join(format!("static_defs_{prefix}.h"));
        let header_file_path = directory.join(&base_path);

        let header_file_src = crate::c_gen::staticlib_header::generate_header_file(
            &prefix,
            module_info,
            &ModuleMetadataSymbolRegistry {
                prefix: prefix.clone(),
            },
            metadata_length,
        );

        std::fs::write(&header_file_path, &header_file_src).map_err(|e| {
            anyhow::anyhow!(
                "could not write static_defs.h for atom {atom_name} in generate-header step: {e}"
            )
        })?;

        atom.header = Some(base_path);
    }

    write_entrypoint(directory, entrypoint)?;

    Ok(())
}

/// Given a directory, links all the objects from the directory appropriately
fn link_exe_from_dir(
    directory: &Path,
    output_path: PathBuf,
    cross_compilation: &CrossCompileSetup,
    additional_libraries: &[String],
    debug: bool,
    atoms: &[(String, Vec<u8>)],
    prefixes: &[String],
) -> anyhow::Result<()> {
    let entrypoint =
        get_entrypoint(directory).with_context(|| anyhow::anyhow!("link exe from dir"))?;

    let prefixes = PrefixMapCompilation::from_input(atoms, prefixes, false)
        .with_context(|| anyhow::anyhow!("link_exe_from_dir"))?;

    let wasmer_main_c = generate_wasmer_main_c(&entrypoint, &prefixes).map_err(|e| {
        anyhow::anyhow!(
            "could not generate wasmer_main.c in dir {}: {e}",
            directory.display()
        )
    })?;

    std::fs::write(directory.join("wasmer_main.c"), wasmer_main_c.as_bytes()).map_err(|e| {
        anyhow::anyhow!(
            "could not write wasmer_main.c in dir {}: {e}",
            directory.display()
        )
    })?;

    let library_path = &cross_compilation.library;

    let mut object_paths = entrypoint
        .atoms
        .iter()
        .filter_map(|a| directory.join(&a.path).canonicalize().ok())
        .collect::<Vec<_>>();

    object_paths.extend(
        entrypoint
            .volumes
            .iter()
            .filter_map(|v| directory.join(&v.obj_file).canonicalize().ok()),
    );

    let zig_triple = utils::triple_to_zig_triple(&cross_compilation.target);
    let include_dirs = entrypoint
        .atoms
        .iter()
        .filter_map(|a| {
            Some(
                directory
                    .join(a.header.as_deref()?)
                    .canonicalize()
                    .ok()?
                    .parent()?
                    .to_path_buf(),
            )
        })
        .collect::<Vec<_>>();

    let mut include_dirs = include_dirs;
    include_dirs.sort();
    include_dirs.dedup();

    // On Windows, cross-compilation to Windows itself with zig does not work due
    // to libunwind and libstdc++ not compiling, so we fake this special case of cross-compilation
    // by falling back to the system compilation + system linker
    if cross_compilation.target == Triple::host()
        && cross_compilation.target.operating_system == OperatingSystem::Windows
    {
        run_c_compile(
            &directory.join("wasmer_main.c"),
            &directory.join("wasmer_main.o"),
            &cross_compilation.target,
            debug,
            &include_dirs,
        )
        .map_err(|e| {
            anyhow::anyhow!(
                "could not run c compile of wasmer_main.c in dir {}: {e}",
                directory.display()
            )
        })?;
    }

    // compilation done, now link
    if cross_compilation.zig_binary_path.is_none()
        || (cross_compilation.target == Triple::host()
            && cross_compilation.target.operating_system == OperatingSystem::Windows)
    {
        #[cfg(not(windows))]
        let linker = "cc";
        #[cfg(windows)]
        let linker = "clang";
        let optimization_flag = "-O2";

        let object_path = match directory.join("wasmer_main.o").canonicalize() {
            Ok(s) => s,
            Err(_) => directory.join("wasmer_main.c"),
        };

        object_paths.push(object_path);

        return link_objects_system_linker(
            library_path,
            linker,
            optimization_flag,
            &include_dirs,
            &object_paths,
            &cross_compilation.target,
            additional_libraries,
            &output_path,
            debug,
        );
    }

    let zig_binary_path = cross_compilation
        .zig_binary_path
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("could not find zig in $PATH {}", directory.display()))?;

    let mut cmd = Command::new(zig_binary_path);
    cmd.arg("build-exe");
    cmd.arg("--verbose-cc");
    cmd.arg("--verbose-link");
    cmd.arg("-target");
    cmd.arg(&zig_triple);

    // On Windows, libstdc++ does not compile with zig due to zig-internal problems,
    // so we link with only -lc
    #[cfg(not(target_os = "windows"))]
    if zig_triple.contains("windows") {
        cmd.arg("-lc++");
    } else {
        cmd.arg("-lc");
    }

    #[cfg(target_os = "windows")]
    cmd.arg("-lc");

    for include_dir in include_dirs {
        cmd.arg("-I");
        cmd.arg(normalize_path(&format!("{}", include_dir.display())));
    }

    let mut include_path = library_path.clone();
    include_path.pop();
    include_path.pop();
    include_path.push("include");
    if !include_path.exists() {
        // Can happen when we got the wrong library_path
        return Err(anyhow::anyhow!("Wasmer include path {} does not exist, maybe library path {} is wrong (expected /lib/libwasmer.a)?", include_path.display(), library_path.display()));
    }
    cmd.arg("-I");
    cmd.arg(normalize_path(&format!("{}", include_path.display())));

    // On Windows, libunwind does not compile, so we have
    // to create binaries without libunwind.
    #[cfg(not(target_os = "windows"))]
    cmd.arg("-lunwind");

    #[cfg(target_os = "windows")]
    if !zig_triple.contains("windows") {
        cmd.arg("-lunwind");
    }

    cmd.arg("-OReleaseSafe");
    cmd.arg("-fno-compiler-rt");
    cmd.arg("-fno-lto");
    #[cfg(target_os = "windows")]
    let out_path = directory.join("wasmer_main.exe");
    #[cfg(not(target_os = "windows"))]
    let out_path = directory.join("wasmer_main");
    cmd.arg(&format!("-femit-bin={}", out_path.display()));
    cmd.args(
        &object_paths
            .iter()
            .map(|o| normalize_path(&format!("{}", o.display())))
            .collect::<Vec<_>>(),
    );
    cmd.arg(&normalize_path(&format!("{}", library_path.display())));
    cmd.arg(normalize_path(&format!(
        "{}",
        directory
            .join("wasmer_main.c")
            .canonicalize()
            .expect("could not find wasmer_main.c / wasmer_main.o")
            .display()
    )));

    if zig_triple.contains("windows") {
        let mut winsdk_path = library_path.clone();
        winsdk_path.pop();
        winsdk_path.pop();
        winsdk_path.push("winsdk");

        let files_winsdk = std::fs::read_dir(winsdk_path)
            .ok()
            .map(|res| {
                res.filter_map(|r| Some(normalize_path(&format!("{}", r.ok()?.path().display()))))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        cmd.args(files_winsdk);
    }

    if zig_triple.contains("macos") {
        // need to link with Security framework when using zig, but zig
        // doesn't include it, so we need to bring a copy of the dtb file
        // which is basicaly the collection of exported symbol for the libs
        let framework = include_bytes!("security_framework.tgz").to_vec();
        // extract files
        let tar = flate2::read::GzDecoder::new(framework.as_slice());
        let mut archive = Archive::new(tar);
        // directory is a temp folder
        archive.unpack(directory)?;
        // add the framework to the link command
        cmd.arg("-framework");
        cmd.arg("Security");
        cmd.arg(format!("-F{}", directory.display()));
    }

    if debug {
        println!("running cmd: {cmd:?}");
        cmd.stdout(Stdio::inherit());
        cmd.stderr(Stdio::inherit());
    }

    let compilation = cmd
        .output()
        .context(anyhow!("Could not execute `zig`: {cmd:?}"))?;

    if !compilation.status.success() {
        return Err(anyhow::anyhow!(String::from_utf8_lossy(
            &compilation.stderr
        )
        .to_string()));
    }

    // remove file if it exists - if not done, can lead to errors on copy
    let output_path_normalized = normalize_path(&format!("{}", output_path.display()));
    let _ = std::fs::remove_file(output_path_normalized);
    std::fs::copy(
        normalize_path(&format!("{}", out_path.display())),
        normalize_path(&format!("{}", output_path.display())),
    )
    .map_err(|e| {
        anyhow::anyhow!(
            "could not copy from {} to {}: {e}",
            normalize_path(&format!("{}", out_path.display())),
            normalize_path(&format!("{}", output_path.display()))
        )
    })?;

    Ok(())
}

/// Link compiled objects using the system linker
#[allow(clippy::too_many_arguments)]
fn link_objects_system_linker(
    libwasmer_path: &Path,
    linker_cmd: &str,
    optimization_flag: &str,
    include_dirs: &[PathBuf],
    object_paths: &[PathBuf],
    target: &Triple,
    additional_libraries: &[String],
    output_path: &Path,
    debug: bool,
) -> Result<(), anyhow::Error> {
    let libwasmer_path = libwasmer_path
        .canonicalize()
        .context("Failed to find libwasmer")?;
    let mut command = Command::new(linker_cmd);
    let mut command = command
        .arg("-Wall")
        .arg(optimization_flag)
        .args(object_paths.iter().map(|path| path.canonicalize().unwrap()))
        .arg(&libwasmer_path);

    if *target != Triple::host() {
        command = command.arg("-target");
        command = command.arg(format!("{}", target));
    }

    for include_dir in include_dirs {
        command = command.arg("-I");
        command = command.arg(normalize_path(&format!("{}", include_dir.display())));
    }
    let mut include_path = libwasmer_path.clone();
    include_path.pop();
    include_path.pop();
    include_path.push("include");
    if !include_path.exists() {
        // Can happen when we got the wrong library_path
        return Err(anyhow::anyhow!("Wasmer include path {} does not exist, maybe library path {} is wrong (expected /lib/libwasmer.a)?", include_path.display(), libwasmer_path.display()));
    }
    command = command.arg("-I");
    command = command.arg(normalize_path(&format!("{}", include_path.display())));

    // Add libraries required per platform.
    // We need userenv, sockets (Ws2_32), advapi32 for some system calls and bcrypt for random numbers.
    let mut additional_libraries = additional_libraries.to_vec();
    if target.operating_system == OperatingSystem::Windows {
        additional_libraries.extend(LINK_SYSTEM_LIBRARIES_WINDOWS.iter().map(|s| s.to_string()));
    } else {
        additional_libraries.extend(LINK_SYSTEM_LIBRARIES_UNIX.iter().map(|s| s.to_string()));
    }
    let link_against_extra_libs = additional_libraries.iter().map(|lib| format!("-l{}", lib));
    let command = command.args(link_against_extra_libs);
    let command = command.arg("-o").arg(output_path);
    if debug {
        println!("{:#?}", command);
    }
    let output = command.output()?;

    if !output.status.success() {
        bail!(
            "linking failed with command line:{:#?} stdout: {}\n\nstderr: {}",
            command,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
    Ok(())
}

/// Generate the wasmer_main.c that links all object files together
/// (depending on the object format / atoms number)
fn generate_wasmer_main_c(
    entrypoint: &Entrypoint,
    prefixes: &PrefixMapCompilation,
) -> Result<String, anyhow::Error> {
    use std::fmt::Write;

    const WASMER_MAIN_C_SOURCE: &str = include_str!("wasmer_create_exe_main.c");

    // always with compile zig + static_defs.h
    let atom_names = entrypoint
        .atoms
        .iter()
        .map(|a| &a.command)
        .collect::<Vec<_>>();

    let c_code_to_add = String::new();
    let mut c_code_to_instantiate = String::new();
    let mut deallocate_module = String::new();
    let mut extra_headers = Vec::new();

    for a in atom_names.iter() {
        let prefix = prefixes
            .get_prefix_for_atom(&utils::normalize_atom_name(a))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "cannot find prefix for atom {a} when generating wasmer_main.c ({:#?})",
                    prefixes
                )
            })?;
        let atom_name = prefix.clone();

        extra_headers.push(format!("#include \"static_defs_{atom_name}.h\""));

        write!(c_code_to_instantiate, "
        wasm_module_t *atom_{atom_name} = wasmer_object_module_new_{atom_name}(store, \"{atom_name}\");
        if (!atom_{atom_name}) {{
            fprintf(stderr, \"Failed to create module from atom \\\"{a}\\\"\\n\");
            print_wasmer_error();
            return -1;
        }}
        ")?;

        write!(deallocate_module, "wasm_module_delete(atom_{atom_name});")?;
    }

    let volumes_str = entrypoint
        .volumes
        .iter()
        .map(|v| utils::normalize_atom_name(&v.name).to_uppercase())
        .map(|uppercase| {
            format!(
                "extern size_t {uppercase}_LENGTH asm(\"{uppercase}_LENGTH\");\r\nextern char {uppercase}_DATA asm(\"{uppercase}_DATA\");"
            )
        })
        .collect::<Vec<_>>();

    let base_str = WASMER_MAIN_C_SOURCE;
    let volumes_str = volumes_str.join("\r\n");
    let return_str = base_str
        .replace(
            "#define WASI",
            if !volumes_str.trim().is_empty() {
                "#define WASI\r\n#define WASI_PIRITA"
            } else {
                "#define WASI"
            },
        )
        .replace("// DECLARE_MODULES", &c_code_to_add)
        .replace("// DECLARE_VOLUMES", &volumes_str)
        .replace(
            "// SET_NUMBER_OF_COMMANDS",
            &format!("number_of_commands = {};", atom_names.len()),
        )
        .replace("// EXTRA_HEADERS", &extra_headers.join("\r\n"))
        .replace("wasm_module_delete(module);", &deallocate_module);

    if atom_names.len() == 1 {
        let prefix = prefixes
            .get_prefix_for_atom(&utils::normalize_atom_name(atom_names[0]))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "cannot find prefix for atom {} when generating wasmer_main.c ({:#?})",
                    &atom_names[0],
                    prefixes
                )
            })?;
        write!(c_code_to_instantiate, "module = atom_{prefix};")?;
    } else {
        for a in atom_names.iter() {
            let prefix = prefixes
                .get_prefix_for_atom(&utils::normalize_atom_name(a))
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "cannot find prefix for atom {a} when generating wasmer_main.c ({:#?})",
                        prefixes
                    )
                })?;
            writeln!(
                c_code_to_instantiate,
                "if (strcmp(selected_atom, \"{a}\") == 0) {{ module = atom_{}; }}",
                prefix
            )?;
        }
    }

    write!(
        c_code_to_instantiate,
        "
    if (!module) {{
        fprintf(stderr, \"No --command given, available commands are:\\n\");
        fprintf(stderr, \"\\n\");
        {commands}
        fprintf(stderr, \"\\n\");
        return -1;
    }}
    ",
        commands = atom_names
            .iter()
            .map(|a| format!("fprintf(stderr, \"    {a}\\n\");"))
            .collect::<Vec<_>>()
            .join("\n")
    )?;

    Ok(return_str.replace("// INSTANTIATE_MODULES", &c_code_to_instantiate))
}

#[allow(dead_code)]
pub(super) mod utils {

    use std::{
        ffi::OsStr,
        path::{Path, PathBuf},
    };

    use anyhow::{anyhow, Context};
    use target_lexicon::{Architecture, Environment, OperatingSystem, Triple};
    use wasmer_types::{CpuFeature, Target};

    use super::{CrossCompile, CrossCompileSetup, UrlOrVersion};

    pub(in crate::commands) fn target_triple_to_target(
        target_triple: &Triple,
        cpu_features: &[CpuFeature],
    ) -> Target {
        let mut features = cpu_features.iter().fold(CpuFeature::set(), |a, b| a | *b);
        // Cranelift requires SSE2, so we have this "hack" for now to facilitate
        // usage
        if target_triple.architecture == Architecture::X86_64 {
            features |= CpuFeature::SSE2;
        }
        Target::new(target_triple.clone(), features)
    }

    pub(in crate::commands) fn get_cross_compile_setup(
        cross_subc: &mut CrossCompile,
        target_triple: &Triple,
        starting_cd: &Path,
        specific_release: Option<UrlOrVersion>,
    ) -> Result<CrossCompileSetup, anyhow::Error> {
        let target = target_triple;

        if let Some(tarball_path) = cross_subc.tarball.as_mut() {
            if tarball_path.is_relative() {
                *tarball_path = starting_cd.join(&tarball_path);
                if !tarball_path.exists() {
                    return Err(anyhow!(
                        "Tarball path `{}` does not exist.",
                        tarball_path.display()
                    ));
                } else if tarball_path.is_dir() {
                    return Err(anyhow!(
                        "Tarball path `{}` is a directory.",
                        tarball_path.display()
                    ));
                }
            }
        }

        let zig_binary_path = if !cross_subc.use_system_linker {
            find_zig_binary(cross_subc.zig_binary_path.as_ref().and_then(|p| {
                if p.is_absolute() {
                    p.canonicalize().ok()
                } else {
                    starting_cd.join(p).canonicalize().ok()
                }
            }))
            .ok()
        } else {
            None
        };

        let library = if let Some(v) = cross_subc.library_path.clone() {
            Some(v.canonicalize().unwrap_or(v))
        } else if let Some(local_tarball) = cross_subc.tarball.as_ref() {
            let (filename, tarball_dir) = find_filename(local_tarball, target)?;
            Some(tarball_dir.join(filename))
        } else {
            let wasmer_cache_dir =
                if *target_triple == Triple::host() && std::env::var("WASMER_DIR").is_ok() {
                    wasmer_registry::WasmerConfig::get_wasmer_dir()
                        .map_err(|e| anyhow!("{e}"))
                        .map(|o| o.join("cache"))
                } else {
                    get_libwasmer_cache_path()
                }
                .ok();

            // check if the tarball for the target already exists locally
            let local_tarball = wasmer_cache_dir.as_ref().and_then(|wc| {
                let wasmer_cache = std::fs::read_dir(wc).ok()?;
                wasmer_cache
                    .filter_map(|e| e.ok())
                    .filter_map(|e| {
                        let path = format!("{}", e.path().display());
                        if path.ends_with(".tar.gz") {
                            Some(e.path())
                        } else {
                            None
                        }
                    })
                    .find(|p| crate::commands::utils::filter_tarball(p, target))
            });

            if let Some(UrlOrVersion::Url(wasmer_release)) = specific_release.as_ref() {
                let tarball = super::http_fetch::download_url(wasmer_release.as_ref())?;
                let (filename, tarball_dir) = find_filename(&tarball, target)?;
                Some(tarball_dir.join(filename))
            } else if let Some(UrlOrVersion::Version(wasmer_release)) = specific_release.as_ref() {
                let release = super::http_fetch::get_release(Some(wasmer_release.clone()))?;
                let tarball = super::http_fetch::download_release(release, target.clone())?;
                let (filename, tarball_dir) = find_filename(&tarball, target)?;
                Some(tarball_dir.join(filename))
            } else if let Some(local_tarball) = local_tarball.as_ref() {
                let (filename, tarball_dir) = find_filename(local_tarball, target)?;
                Some(tarball_dir.join(filename))
            } else {
                let release = super::http_fetch::get_release(None)?;
                let tarball = super::http_fetch::download_release(release, target.clone())?;
                let (filename, tarball_dir) = find_filename(&tarball, target)?;
                Some(tarball_dir.join(filename))
            }
        };

        let library = library.ok_or_else(|| anyhow!("libwasmer.a / wasmer.lib not found"))?;

        let ccs = CrossCompileSetup {
            target: target.clone(),
            zig_binary_path,
            library,
        };
        Ok(ccs)
    }

    pub(super) fn filter_tarball(p: &Path, target: &Triple) -> bool {
        filter_tarball_internal(p, target).unwrap_or(false)
    }

    fn filter_tarball_internal(p: &Path, target: &Triple) -> Option<bool> {
        if !p.file_name()?.to_str()?.ends_with(".tar.gz") {
            return None;
        }

        if target.environment == Environment::Musl && !p.file_name()?.to_str()?.contains("musl")
            || p.file_name()?.to_str()?.contains("musl") && target.environment != Environment::Musl
        {
            return None;
        }

        if let Architecture::Aarch64(_) = target.architecture {
            if !(p.file_name()?.to_str()?.contains("aarch64")
                || p.file_name()?.to_str()?.contains("arm64"))
            {
                return None;
            }
        }

        if let Architecture::X86_64 = target.architecture {
            if target.operating_system == OperatingSystem::Windows {
                if !p.file_name()?.to_str()?.contains("gnu64") {
                    return None;
                }
            } else if !(p.file_name()?.to_str()?.contains("x86_64")
                || p.file_name()?.to_str()?.contains("amd64"))
            {
                return None;
            }
        }

        if let OperatingSystem::Windows = target.operating_system {
            if !p.file_name()?.to_str()?.contains("windows") {
                return None;
            }
        }

        if let OperatingSystem::Darwin = target.operating_system {
            if !(p.file_name()?.to_str()?.contains("apple")
                || p.file_name()?.to_str()?.contains("darwin"))
            {
                return None;
            }
        }

        if let OperatingSystem::Linux = target.operating_system {
            if !p.file_name()?.to_str()?.contains("linux") {
                return None;
            }
        }

        Some(true)
    }

    pub(super) fn find_filename(
        local_tarball: &Path,
        target: &Triple,
    ) -> Result<(PathBuf, PathBuf), anyhow::Error> {
        let target_file_path = local_tarball
            .parent()
            .and_then(|parent| Some(parent.join(local_tarball.file_stem()?)))
            .unwrap_or_else(|| local_tarball.to_path_buf());

        let target_file_path = target_file_path
            .parent()
            .and_then(|parent| Some(parent.join(target_file_path.file_stem()?)))
            .unwrap_or_else(|| target_file_path.clone());

        std::fs::create_dir_all(&target_file_path)
            .map_err(|e| anyhow!("{e}"))
            .with_context(|| anyhow!("{}", target_file_path.display()))?;
        let files =
            super::http_fetch::untar(local_tarball, &target_file_path).with_context(|| {
                anyhow!(
                    "{} -> {}",
                    local_tarball.display(),
                    target_file_path.display()
                )
            })?;
        let tarball_dir = target_file_path.canonicalize().unwrap_or(target_file_path);
        let file = find_libwasmer_in_files(target, &files)?;
        Ok((file, tarball_dir))
    }

    fn find_libwasmer_in_files(
        target: &Triple,
        files: &[PathBuf],
    ) -> Result<PathBuf, anyhow::Error> {
        let target_files = &[
            OsStr::new("libwasmer-headless.a"),
            OsStr::new("wasmer-headless.lib"),
            OsStr::new("libwasmer.a"),
            OsStr::new("wasmer.lib"),
        ];
        target_files
        .iter()
        .find_map(|q| {
            files.iter().find(|f| f.file_name() == Some(*q))
        })
        .cloned()
        .ok_or_else(|| {
            anyhow!("Could not find libwasmer.a for {} target in the provided tarball path (files = {files:#?})", target)
        })
    }

    pub(super) fn normalize_atom_name(s: &str) -> String {
        s.chars()
            .filter_map(|c| {
                if char::is_alphabetic(c) {
                    Some(c)
                } else if c == '-' || c == '_' {
                    Some('_')
                } else {
                    None
                }
            })
            .collect()
    }

    pub(super) fn triple_to_zig_triple(target_triple: &Triple) -> String {
        let arch = match target_triple.architecture {
            wasmer_types::Architecture::X86_64 => "x86_64".into(),
            wasmer_types::Architecture::Aarch64(wasmer_types::Aarch64Architecture::Aarch64) => {
                "aarch64".into()
            }
            v => v.to_string(),
        };
        let os = match target_triple.operating_system {
            wasmer_types::OperatingSystem::Linux => "linux".into(),
            wasmer_types::OperatingSystem::Darwin => "macos".into(),
            wasmer_types::OperatingSystem::Windows => "windows".into(),
            v => v.to_string(),
        };
        let env = match target_triple.environment {
            wasmer_types::Environment::Musl => "musl",
            wasmer_types::Environment::Gnu => "gnu",
            wasmer_types::Environment::Msvc => "msvc",
            _ => "none",
        };
        format!("{}-{}-{}", arch, os, env)
    }

    pub(super) fn get_wasmer_dir() -> anyhow::Result<PathBuf> {
        wasmer_registry::WasmerConfig::get_wasmer_dir().map_err(|e| anyhow!("{e}"))
    }

    pub(super) fn get_wasmer_include_directory() -> anyhow::Result<PathBuf> {
        let mut path = get_wasmer_dir()?;
        if path.clone().join("wasmer.h").exists() {
            return Ok(path);
        }
        path.push("include");
        if !path.clone().join("wasmer.h").exists() {
            if !path.exists() {
                return Err(anyhow!("WASMER_DIR path {} does not exist", path.display()));
            }
            println!(
                "wasmer.h does not exist in {}, will probably default to the system path",
                path.canonicalize().unwrap().display()
            );
        }
        Ok(path)
    }

    /// path to the static libwasmer
    pub(super) fn get_libwasmer_path() -> anyhow::Result<PathBuf> {
        let path = get_wasmer_dir()?;

        // TODO: prefer headless Wasmer if/when it's a separate library.
        #[cfg(not(windows))]
        let libwasmer_static_name = "libwasmer.a";
        #[cfg(windows)]
        let libwasmer_static_name = "libwasmer.lib";

        if path.exists() && path.join(libwasmer_static_name).exists() {
            Ok(path.join(libwasmer_static_name))
        } else {
            Ok(path.join("lib").join(libwasmer_static_name))
        }
    }

    /// path to library tarball cache dir
    pub(super) fn get_libwasmer_cache_path() -> anyhow::Result<PathBuf> {
        let mut path = get_wasmer_dir()?;
        path.push("cache");
        std::fs::create_dir_all(&path)?;
        Ok(path)
    }

    pub(super) fn get_zig_exe_str() -> &'static str {
        #[cfg(target_os = "windows")]
        {
            "zig.exe"
        }
        #[cfg(not(target_os = "windows"))]
        {
            "zig"
        }
    }

    pub(super) fn find_zig_binary(path: Option<PathBuf>) -> Result<PathBuf, anyhow::Error> {
        use std::env::split_paths;
        #[cfg(unix)]
        use std::os::unix::ffi::OsStrExt;
        let path_var = std::env::var("PATH").unwrap_or_default();
        #[cfg(unix)]
        let system_path_var = std::process::Command::new("getconf")
            .args(["PATH"])
            .output()
            .map(|output| output.stdout)
            .unwrap_or_default();
        let retval = if let Some(p) = path {
            if p.exists() {
                p
            } else {
                return Err(anyhow!("Could not find `zig` binary in {}.", p.display()));
            }
        } else {
            let mut retval = None;
            for mut p in split_paths(&path_var).chain(split_paths(
                #[cfg(unix)]
                {
                    &OsStr::from_bytes(&system_path_var[..])
                },
                #[cfg(not(unix))]
                {
                    OsStr::new("")
                },
            )) {
                p.push(get_zig_exe_str());
                if p.exists() {
                    retval = Some(p);
                    break;
                }
            }
            retval.ok_or_else(|| anyhow!("Could not find `zig` binary in PATH."))?
        };

        let version = std::process::Command::new(&retval)
            .arg("version")
            .output()
            .with_context(|| {
                format!(
                    "Could not execute `zig` binary at path `{}`",
                    retval.display()
                )
            })?
            .stdout;
        let version_slice = if let Some(pos) = version
            .iter()
            .position(|c| !(c.is_ascii_digit() || (*c == b'.')))
        {
            &version[..pos]
        } else {
            &version[..]
        };

        let version_slice = String::from_utf8_lossy(version_slice);
        let version_semver = semver::Version::parse(&version_slice)
            .map_err(|e| anyhow!("could not parse zig version: {version_slice}: {e}"))?;

        if version_semver < semver::Version::parse("0.10.0").unwrap() {
            Err(anyhow!("`zig` binary in PATH (`{}`) is not a new enough version (`{version_slice}`): please use version `0.10.0` or newer.", retval.display()))
        } else {
            Ok(retval)
        }
    }

    #[test]
    fn test_filter_tarball() {
        use std::str::FromStr;
        let test_paths = [
            "/test/wasmer-darwin-amd64.tar.gz",
            "/test/wasmer-darwin-arm64.tar.gz",
            "/test/wasmer-linux-aarch64.tar.gz",
            "/test/wasmer-linux-amd64.tar.gz",
            "/test/wasmer-linux-musl-amd64.tar.gz",
            "/test/wasmer-windows-amd64.tar.gz",
            "/test/wasmer-windows-gnu64.tar.gz",
            "/test/wasmer-windows.exe",
        ];

        let paths = test_paths.iter().map(Path::new).collect::<Vec<_>>();
        assert_eq!(
            paths
                .iter()
                .filter(|p| crate::commands::utils::filter_tarball(
                    p,
                    &Triple::from_str("x86_64-windows").unwrap()
                ))
                .collect::<Vec<_>>(),
            vec![&Path::new("/test/wasmer-windows-gnu64.tar.gz")],
        );

        let paths = test_paths.iter().map(Path::new).collect::<Vec<_>>();
        assert_eq!(
            paths
                .iter()
                .filter(|p| crate::commands::utils::filter_tarball(
                    p,
                    &Triple::from_str("x86_64-windows-gnu").unwrap()
                ))
                .collect::<Vec<_>>(),
            vec![&Path::new("/test/wasmer-windows-gnu64.tar.gz")],
        );

        let paths = test_paths.iter().map(Path::new).collect::<Vec<_>>();
        assert_eq!(
            paths
                .iter()
                .filter(|p| crate::commands::utils::filter_tarball(
                    p,
                    &Triple::from_str("x86_64-windows-msvc").unwrap()
                ))
                .collect::<Vec<_>>(),
            vec![&Path::new("/test/wasmer-windows-gnu64.tar.gz")],
        );

        assert_eq!(
            paths
                .iter()
                .filter(|p| crate::commands::utils::filter_tarball(
                    p,
                    &Triple::from_str("x86_64-darwin").unwrap()
                ))
                .collect::<Vec<_>>(),
            vec![&Path::new("/test/wasmer-darwin-amd64.tar.gz")],
        );

        assert_eq!(
            paths
                .iter()
                .filter(|p| crate::commands::utils::filter_tarball(
                    p,
                    &Triple::from_str("x86_64-unknown-linux-gnu").unwrap()
                ))
                .collect::<Vec<_>>(),
            vec![&Path::new("/test/wasmer-linux-amd64.tar.gz")],
        );

        assert_eq!(
            paths
                .iter()
                .filter(|p| crate::commands::utils::filter_tarball(
                    p,
                    &Triple::from_str("x86_64-linux-gnu").unwrap()
                ))
                .collect::<Vec<_>>(),
            vec![&Path::new("/test/wasmer-linux-amd64.tar.gz")],
        );

        assert_eq!(
            paths
                .iter()
                .filter(|p| crate::commands::utils::filter_tarball(
                    p,
                    &Triple::from_str("aarch64-linux-gnu").unwrap()
                ))
                .collect::<Vec<_>>(),
            vec![&Path::new("/test/wasmer-linux-aarch64.tar.gz")],
        );

        assert_eq!(
            paths
                .iter()
                .filter(|p| crate::commands::utils::filter_tarball(
                    p,
                    &Triple::from_str("x86_64-windows-gnu").unwrap()
                ))
                .collect::<Vec<_>>(),
            vec![&Path::new("/test/wasmer-windows-gnu64.tar.gz")],
        );

        assert_eq!(
            paths
                .iter()
                .filter(|p| crate::commands::utils::filter_tarball(
                    p,
                    &Triple::from_str("aarch64-darwin").unwrap()
                ))
                .collect::<Vec<_>>(),
            vec![&Path::new("/test/wasmer-darwin-arm64.tar.gz")],
        );
    }

    #[test]
    fn test_normalize_atom_name() {
        assert_eq!(
            normalize_atom_name("atom-name-with-dash"),
            "atom_name_with_dash".to_string()
        );
    }
}

mod http_fetch {
    use std::path::Path;

    use anyhow::{anyhow, Context, Result};

    pub(super) fn get_release(
        release_version: Option<semver::Version>,
    ) -> Result<serde_json::Value> {
        let uri = "https://api.github.com/repos/wasmerio/wasmer/releases";

        // Increases rate-limiting in GitHub CI
        let auth = std::env::var("GITHUB_TOKEN");

        let client = reqwest::blocking::Client::new();
        let mut req = client.get(uri);
        if let Ok(token) = auth {
            req = req.header("Authorization", &format!("Bearer {token}"));
        }

        let response = req
            .header("User-Agent", "wasmerio")
            .header("Accept", "application/vnd.github.v3+json")
            .send()
            .map_err(anyhow::Error::new)
            .context("Could not lookup wasmer repository on Github.")?;

        let status = response.status();

        let body = response
            .bytes()
            .map_err(anyhow::Error::new)
            .context("Could not retrieve wasmer release history body")?;

        if status != reqwest::StatusCode::OK {
            log::warn!(
                "Warning: Github API replied with non-200 status code: {}. Response: {}",
                status,
                String::from_utf8_lossy(&body),
            );
        }

        let mut response = serde_json::from_slice::<serde_json::Value>(&body)?;

        if let Some(releases) = response.as_array_mut() {
            releases.retain(|r| {
                r["tag_name"].is_string() && !r["tag_name"].as_str().unwrap().is_empty()
            });
            releases.sort_by_cached_key(|r| r["tag_name"].as_str().unwrap_or_default().to_string());
            match release_version {
                Some(specific_version) => {
                    let mut all_versions = Vec::new();
                    for r in releases.iter() {
                        if r["tag_name"].as_str().unwrap_or_default()
                            == specific_version.to_string()
                        {
                            return Ok(r.clone());
                        } else {
                            all_versions
                                .push(r["tag_name"].as_str().unwrap_or_default().to_string());
                        }
                    }
                    return Err(anyhow::anyhow!(
                        "could not find release version {}, available versions are: {}",
                        specific_version,
                        all_versions.join(", ")
                    ));
                }
                None => {
                    if let Some(latest) = releases.pop() {
                        return Ok(latest);
                    }
                }
            }
        }

        Err(anyhow!(
            "Could not get expected Github API response.\n\nReason: response format is not recognized:\n{response:#?}",
        ))
    }

    pub(super) fn download_release(
        mut release: serde_json::Value,
        target_triple: wasmer::Triple,
    ) -> Result<std::path::PathBuf> {
        // Test if file has been already downloaded
        if let Ok(mut cache_path) = super::utils::get_libwasmer_cache_path() {
            let paths = std::fs::read_dir(&cache_path).and_then(|r| {
                r.map(|res| res.map(|e| e.path()))
                    .collect::<Result<Vec<_>, std::io::Error>>()
            });

            if let Ok(mut entries) = paths {
                entries.retain(|p| p.to_str().map(|p| p.ends_with(".tar.gz")).unwrap_or(false));
                entries.retain(|p| super::utils::filter_tarball(p, &target_triple));
                if !entries.is_empty() {
                    cache_path.push(&entries[0]);
                    if cache_path.exists() {
                        eprintln!(
                            "Using cached tarball to cache path `{}`.",
                            cache_path.display()
                        );
                        return Ok(cache_path);
                    }
                }
            }
        }

        let assets = match release["assets"].as_array_mut() {
            Some(s) => s,
            None => {
                return Err(anyhow!(
                    "GitHub API: no [assets] array in JSON response for latest releases"
                ));
            }
        };

        assets.retain(|a| {
            let name = match a["name"].as_str() {
                Some(s) => s,
                None => return false,
            };
            super::utils::filter_tarball(Path::new(&name), &target_triple)
        });

        if assets.len() != 1 {
            return Err(anyhow!(
                "GitHub API: more that one release selected for target {target_triple}: {assets:?}"
            ));
        }

        let browser_download_url = if let Some(url) = assets[0]["browser_download_url"].as_str() {
            url.to_string()
        } else {
            return Err(anyhow!(
                "Could not get download url from Github API response."
            ));
        };

        download_url(&browser_download_url)
    }

    pub(crate) fn download_url(
        browser_download_url: &str,
    ) -> Result<std::path::PathBuf, anyhow::Error> {
        let filename = browser_download_url
            .split('/')
            .last()
            .unwrap_or("output")
            .to_string();

        let download_tempdir = tempfile::TempDir::new()?;
        let download_path = download_tempdir.path().join(&filename);

        let mut file = std::fs::File::create(&download_path)?;
        log::debug!(
            "Downloading {} to {}",
            browser_download_url,
            download_path.display()
        );

        let mut response = reqwest::blocking::Client::builder()
            .redirect(reqwest::redirect::Policy::limited(10))
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(anyhow::Error::new)
            .context("Could not lookup wasmer artifact on Github.")?
            .get(browser_download_url)
            .send()
            .map_err(anyhow::Error::new)
            .context("Could not lookup wasmer artifact on Github.")?;

        response
            .copy_to(&mut file)
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        match super::utils::get_libwasmer_cache_path() {
            Ok(mut cache_path) => {
                cache_path.push(&filename);
                if let Err(err) = std::fs::copy(&download_path, &cache_path) {
                    eprintln!(
                        "Could not store tarball to cache path `{}`: {}",
                        cache_path.display(),
                        err
                    );
                    Err(anyhow!(
                        "Could not copy from {} to {}",
                        download_path.display(),
                        cache_path.display()
                    ))
                } else {
                    eprintln!("Cached tarball to cache path `{}`.", cache_path.display());
                    Ok(cache_path)
                }
            }
            Err(err) => {
                eprintln!(
                    "Could not determine cache path for downloaded binaries.: {}",
                    err
                );
                Err(anyhow!("Could not determine libwasmer cache path"))
            }
        }
    }

    use std::path::PathBuf;

    pub(crate) fn list_dir(target: &Path) -> Vec<PathBuf> {
        use walkdir::WalkDir;
        WalkDir::new(target)
            .into_iter()
            .filter_map(|e| e.ok())
            .map(|entry| entry.path().to_path_buf())
            .collect()
    }

    pub(super) fn untar(tarball: &Path, target: &Path) -> Result<Vec<PathBuf>> {
        let _ = std::fs::remove_dir(target);
        wasmer_registry::try_unpack_targz(tarball, target, false)?;
        Ok(list_dir(target))
    }
}
