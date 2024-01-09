use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use anyhow::Context;
use cargo_metadata::{CargoOpt, MetadataCommand};
use clap::Parser;
use semver::VersionReq;
use wasmer_registry::wasmer_env::WasmerEnv;

static NOTE: &str = "# See more keys and definitions at https://docs.wasmer.io/registry/manifest";

const NEWLINE: &str = if cfg!(windows) { "\r\n" } else { "\n" };

/// CLI args for the `wasmer init` command
#[derive(Debug, Parser)]
pub struct Init {
    #[clap(flatten)]
    env: WasmerEnv,

    /// Initialize wasmer.toml for a library package
    #[clap(long, group = "crate-type")]
    pub lib: bool,
    /// Initialize wasmer.toml for a binary package
    #[clap(long, group = "crate-type")]
    pub bin: bool,
    /// Initialize an empty wasmer.toml
    #[clap(long, group = "crate-type")]
    pub empty: bool,
    /// Force overwriting the wasmer.toml, even if it already exists
    #[clap(long)]
    pub overwrite: bool,
    /// Don't display debug output
    #[clap(long)]
    pub quiet: bool,
    /// Namespace to init with, default = current logged in user or _
    #[clap(long)]
    pub namespace: Option<String>,
    /// Package name to init with, default = Cargo.toml name or current directory name
    #[clap(long)]
    pub package_name: Option<String>,
    /// Version of the initialized package
    #[clap(long)]
    pub version: Option<semver::Version>,
    /// If the `manifest-path` is a Cargo.toml, use that file to initialize the wasmer.toml
    #[clap(long)]
    pub manifest_path: Option<PathBuf>,
    /// Add default dependencies for common packages
    #[clap(long, value_enum)]
    pub template: Option<Template>,
    /// Include file paths into the target container filesystem
    #[clap(long)]
    pub include: Vec<String>,
    /// Directory of the output file name. wasmer init will error if the target dir
    /// already contains a wasmer.toml. Also sets the package name.
    #[clap(name = "PACKAGE_PATH")]
    pub out: Option<PathBuf>,
}

/// What template to use for the initialized wasmer.toml
#[derive(Debug, PartialEq, Eq, Copy, Clone, clap::ValueEnum)]
pub enum Template {
    /// Add dependency on Python
    Python,
    /// Add dependency on JS
    Js,
}

#[derive(Debug, PartialEq, Copy, Clone)]
enum BinOrLib {
    Bin,
    Lib,
    Empty,
}

// minimal version of the Cargo.toml [package] section
#[derive(Debug, Clone)]
struct MiniCargoTomlPackage {
    cargo_toml_path: PathBuf,
    name: String,
    version: semver::Version,
    description: Option<String>,
    homepage: Option<String>,
    repository: Option<String>,
    license: Option<String>,
    readme: Option<PathBuf>,
    license_file: Option<PathBuf>,
    #[allow(dead_code)]
    workspace_root: PathBuf,
    #[allow(dead_code)]
    build_dir: PathBuf,
}

static WASMER_TOML_NAME: &str = "wasmer.toml";

impl Init {
    /// `wasmer init` execution
    pub fn execute(&self) -> Result<(), anyhow::Error> {
        let bin_or_lib = self.get_bin_or_lib()?;

        // See if the directory has a Cargo.toml file, if yes, copy the license / readme, etc.
        let manifest_path = match self.manifest_path.as_ref() {
            Some(s) => s.clone(),
            None => {
                let cargo_toml_path = self
                    .out
                    .clone()
                    .unwrap_or_else(|| std::env::current_dir().unwrap())
                    .join("Cargo.toml");
                cargo_toml_path
                    .canonicalize()
                    .unwrap_or_else(|_| cargo_toml_path.clone())
            }
        };

        let cargo_toml = if manifest_path.exists() {
            parse_cargo_toml(&manifest_path).ok()
        } else {
            None
        };

        let (fallback_package_name, target_file) = self.target_file()?;

        if target_file.exists() && !self.overwrite {
            anyhow::bail!(
                "wasmer project already initialized in {}",
                target_file.display(),
            );
        }

        let constructed_manifest = construct_manifest(
            cargo_toml.as_ref(),
            &fallback_package_name,
            self.package_name.as_deref(),
            &target_file,
            &manifest_path,
            bin_or_lib,
            self.namespace.clone(),
            self.version.clone(),
            self.template.as_ref(),
            self.include.as_slice(),
            self.quiet,
            self.env.dir(),
        )?;

        if let Some(parent) = target_file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        // generate the wasmer.toml and exit
        Self::write_wasmer_toml(&target_file, &constructed_manifest)
    }

    /// Writes the metadata to a wasmer.toml file, making sure we include the
    /// [`NOTE`] so people get a link to the registry docs.
    fn write_wasmer_toml(
        path: &PathBuf,
        toml: &wasmer_toml::Manifest,
    ) -> Result<(), anyhow::Error> {
        let toml_string = toml::to_string_pretty(&toml)?;

        let mut resulting_string = String::new();
        let mut note_inserted = false;

        for line in toml_string.lines() {
            resulting_string.push_str(line);

            if !note_inserted && line.is_empty() {
                // We've found an empty line after the initial [package]
                // section. Let's add our note here.
                resulting_string.push_str(NEWLINE);
                resulting_string.push_str(NOTE);
                resulting_string.push_str(NEWLINE);
                note_inserted = true;
            }
            resulting_string.push_str(NEWLINE);
        }

        if !note_inserted {
            // Make sure the note still ends up at the end of the file.
            resulting_string.push_str(NEWLINE);
            resulting_string.push_str(NOTE);
            resulting_string.push_str(NEWLINE);
            resulting_string.push_str(NEWLINE);
        }

        std::fs::write(path, resulting_string)
            .with_context(|| format!("Unable to write to \"{}\"", path.display()))?;

        Ok(())
    }

    fn target_file(&self) -> Result<(String, PathBuf), anyhow::Error> {
        match self.out.as_ref() {
            None => {
                let current_dir = std::env::current_dir()?;
                let package_name = self
                    .package_name
                    .clone()
                    .or_else(|| {
                        current_dir
                            .canonicalize()
                            .ok()?
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .map(|s| s.to_string())
                    })
                    .ok_or_else(|| anyhow::anyhow!("no current dir name"))?;
                Ok((package_name, current_dir.join(WASMER_TOML_NAME)))
            }
            Some(s) => {
                std::fs::create_dir_all(s)
                    .map_err(|e| anyhow::anyhow!("{e}"))
                    .with_context(|| anyhow::anyhow!("{}", s.display()))?;
                let package_name = self
                    .package_name
                    .clone()
                    .or_else(|| {
                        s.canonicalize()
                            .ok()?
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .map(|s| s.to_string())
                    })
                    .ok_or_else(|| anyhow::anyhow!("no dir name"))?;
                Ok((package_name, s.join(WASMER_TOML_NAME)))
            }
        }
    }

    fn get_filesystem_mapping(include: &[String]) -> impl Iterator<Item = (String, PathBuf)> + '_ {
        include.iter().map(|path| {
            if path == "." || path == "/" {
                return ("/".to_string(), Path::new("/").to_path_buf());
            }

            let key = format!("./{path}");
            let value = PathBuf::from(format!("/{path}"));

            (key, value)
        })
    }

    fn get_command(
        modules: &[wasmer_toml::Module],
        bin_or_lib: BinOrLib,
    ) -> Vec<wasmer_toml::Command> {
        match bin_or_lib {
            BinOrLib::Bin => modules
                .iter()
                .map(|m| {
                    wasmer_toml::Command::V2(wasmer_toml::CommandV2 {
                        name: m.name.clone(),
                        module: wasmer_toml::ModuleReference::CurrentPackage {
                            module: m.name.clone(),
                        },
                        runner: "wasi".to_string(),
                        annotations: None,
                    })
                })
                .collect(),
            BinOrLib::Lib | BinOrLib::Empty => Vec::new(),
        }
    }

    /// Returns the dependencies based on the `--template` flag
    fn get_dependencies(template: Option<&Template>) -> HashMap<String, VersionReq> {
        let mut map = HashMap::default();

        match template {
            Some(Template::Js) => {
                map.insert("quickjs/quickjs".to_string(), VersionReq::STAR);
            }
            Some(Template::Python) => {
                map.insert("python/python".to_string(), VersionReq::STAR);
            }
            _ => {}
        }

        map
    }

    // Returns whether the template for the wasmer.toml should be a binary, a library or an empty file
    fn get_bin_or_lib(&self) -> Result<BinOrLib, anyhow::Error> {
        match (self.empty, self.bin, self.lib) {
            (true, false, false) => Ok(BinOrLib::Empty),
            (false, true, false) => Ok(BinOrLib::Bin),
            (false, false, true) => Ok(BinOrLib::Lib),
            (false, false, false) => Ok(BinOrLib::Bin),
            _ => anyhow::bail!("Only one of --bin, --lib, or --empty can be provided"),
        }
    }

    /// Get bindings returns the first .wai / .wit file found and
    /// optionally takes a warning callback that is triggered when > 1 .wai files are found
    fn get_bindings(target_file: &Path, bin_or_lib: BinOrLib) -> Option<GetBindingsResult> {
        match bin_or_lib {
            BinOrLib::Bin | BinOrLib::Empty => None,
            BinOrLib::Lib => target_file.parent().and_then(|parent| {
                let all_bindings = walkdir::WalkDir::new(parent)
                    .min_depth(1)
                    .max_depth(3)
                    .follow_links(false)
                    .into_iter()
                    .filter_map(|e| e.ok())
                    .filter_map(|e| {
                        let is_wit = e.path().extension().and_then(|s| s.to_str()) == Some(".wit");
                        let is_wai = e.path().extension().and_then(|s| s.to_str()) == Some(".wai");
                        if is_wit {
                            Some(wasmer_toml::Bindings::Wit(wasmer_toml::WitBindings {
                                wit_exports: e.path().to_path_buf(),
                                wit_bindgen: semver::Version::parse("0.1.0").unwrap(),
                            }))
                        } else if is_wai {
                            Some(wasmer_toml::Bindings::Wai(wasmer_toml::WaiBindings {
                                exports: None,
                                imports: vec![e.path().to_path_buf()],
                                wai_version: semver::Version::parse("0.2.0").unwrap(),
                            }))
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();

                if all_bindings.is_empty() {
                    None
                } else if all_bindings.len() == 1 {
                    Some(GetBindingsResult::OneBinding(all_bindings[0].clone()))
                } else {
                    Some(GetBindingsResult::MultiBindings(all_bindings))
                }
            }),
        }
    }
}

enum GetBindingsResult {
    OneBinding(wasmer_toml::Bindings),
    MultiBindings(Vec<wasmer_toml::Bindings>),
}

impl GetBindingsResult {
    fn first_binding(&self) -> Option<wasmer_toml::Bindings> {
        match self {
            Self::OneBinding(s) => Some(s.clone()),
            Self::MultiBindings(s) => s.get(0).cloned(),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn construct_manifest(
    cargo_toml: Option<&MiniCargoTomlPackage>,
    fallback_package_name: &String,
    package_name: Option<&str>,
    target_file: &Path,
    manifest_path: &Path,
    bin_or_lib: BinOrLib,
    namespace: Option<String>,
    version: Option<semver::Version>,
    template: Option<&Template>,
    include_fs: &[String],
    quiet: bool,
    wasmer_dir: &Path,
) -> Result<wasmer_toml::Manifest, anyhow::Error> {
    if let Some(ct) = cargo_toml.as_ref() {
        let msg = format!(
            "NOTE: Initializing wasmer.toml file with metadata from Cargo.toml{NEWLINE}  -> {}",
            ct.cargo_toml_path.display()
        );
        if !quiet {
            println!("{msg}");
        }
        log::warn!("{msg}");
    }

    let package_name = package_name.unwrap_or_else(|| {
        cargo_toml
            .as_ref()
            .map(|p| &p.name)
            .unwrap_or(fallback_package_name)
    });
    let namespace = namespace.or_else(|| {
        wasmer_registry::whoami(wasmer_dir, None, None)
            .ok()
            .map(|o| o.1)
    });
    let version = version.unwrap_or_else(|| {
        cargo_toml
            .as_ref()
            .map(|t| t.version.clone())
            .unwrap_or_else(|| semver::Version::parse("0.1.0").unwrap())
    });
    let license = cargo_toml.as_ref().and_then(|t| t.license.clone());
    let license_file = cargo_toml.as_ref().and_then(|t| t.license_file.clone());
    let readme = cargo_toml.as_ref().and_then(|t| t.readme.clone());
    let repository = cargo_toml.as_ref().and_then(|t| t.repository.clone());
    let homepage = cargo_toml.as_ref().and_then(|t| t.homepage.clone());
    let description = cargo_toml
        .as_ref()
        .and_then(|t| t.description.clone())
        .unwrap_or_else(|| format!("Description for package {package_name}"));

    let default_abi = wasmer_toml::Abi::Wasi;
    let bindings = Init::get_bindings(target_file, bin_or_lib);

    if let Some(GetBindingsResult::MultiBindings(m)) = bindings.as_ref() {
        let found = m
            .iter()
            .map(|m| match m {
                wasmer_toml::Bindings::Wit(wb) => {
                    format!("found: {}", serde_json::to_string(wb).unwrap_or_default())
                }
                wasmer_toml::Bindings::Wai(wb) => {
                    format!("found: {}", serde_json::to_string(wb).unwrap_or_default())
                }
            })
            .collect::<Vec<_>>()
            .join("\r\n");

        let msg = vec![
            String::new(),
            "    It looks like your project contains multiple *.wai files.".to_string(),
            "    Make sure you update the [[module.bindings]] appropriately".to_string(),
            String::new(),
            found,
        ];
        let msg = msg.join("\r\n");
        if !quiet {
            println!("{msg}");
        }
        log::warn!("{msg}");
    }

    let module_source = cargo_toml
        .as_ref()
        .map(|p| {
            // Normalize the path to /target/release to be relative to the parent of the Cargo.toml
            let outpath = p
                .build_dir
                .join("release")
                .join(format!("{package_name}.wasm"));
            let canonicalized_outpath = outpath.canonicalize().unwrap_or(outpath);
            let outpath_str =
                crate::common::normalize_path(&canonicalized_outpath.display().to_string());
            let manifest_canonicalized = crate::common::normalize_path(
                &manifest_path
                    .parent()
                    .and_then(|p| p.canonicalize().ok())
                    .unwrap_or_else(|| manifest_path.to_path_buf())
                    .display()
                    .to_string(),
            );
            let diff = outpath_str
                .strip_prefix(&manifest_canonicalized)
                .unwrap_or(&outpath_str)
                .replace('\\', "/");
            // Format in UNIX fashion (forward slashes)
            let relative_str = diff.strip_prefix('/').unwrap_or(&diff);
            Path::new(&relative_str).to_path_buf()
        })
        .unwrap_or_else(|| Path::new(&format!("{package_name}.wasm")).to_path_buf());

    let modules = vec![wasmer_toml::Module {
        name: package_name.to_string(),
        source: module_source,
        kind: None,
        abi: default_abi,
        bindings: bindings.as_ref().and_then(|b| b.first_binding()),
        interfaces: Some({
            let mut map = HashMap::new();
            map.insert("wasi".to_string(), "0.1.0-unstable".to_string());
            map
        }),
    }];

    let mut pkg = wasmer_toml::Package::builder(
        if let Some(s) = namespace {
            format!("{s}/{package_name}")
        } else {
            package_name.to_string()
        },
        version,
        description,
    );

    if let Some(license) = license {
        pkg.license(license);
    }
    if let Some(license_file) = license_file {
        pkg.license_file(license_file);
    }
    if let Some(readme) = readme {
        pkg.readme(readme);
    }
    if let Some(repository) = repository {
        pkg.repository(repository);
    }
    if let Some(homepage) = homepage {
        pkg.homepage(homepage);
    }
    let pkg = pkg.build()?;

    let mut manifest = wasmer_toml::Manifest::builder(pkg);
    manifest
        .dependencies(Init::get_dependencies(template))
        .commands(Init::get_command(&modules, bin_or_lib))
        .fs(Init::get_filesystem_mapping(include_fs).collect());
    match bin_or_lib {
        BinOrLib::Bin | BinOrLib::Lib => {
            manifest.modules(modules);
        }
        BinOrLib::Empty => {}
    }
    let manifest = manifest.build()?;

    Ok(manifest)
}
fn parse_cargo_toml(manifest_path: &PathBuf) -> Result<MiniCargoTomlPackage, anyhow::Error> {
    let mut metadata = MetadataCommand::new();
    metadata.manifest_path(manifest_path);
    metadata.no_deps();
    metadata.features(CargoOpt::AllFeatures);

    let metadata = metadata.exec().with_context(|| {
        format!(
            "Unable to load metadata from \"{}\"",
            manifest_path.display()
        )
    })?;

    let package = metadata
        .root_package()
        .ok_or_else(|| anyhow::anyhow!("no root package found in cargo metadata"))
        .context(anyhow::anyhow!("{}", manifest_path.display()))?;

    Ok(MiniCargoTomlPackage {
        cargo_toml_path: manifest_path.clone(),
        name: package.name.clone(),
        version: package.version.clone(),
        description: package.description.clone(),
        homepage: package.homepage.clone(),
        repository: package.repository.clone(),
        license: package.license.clone(),
        readme: package.readme.clone().map(|s| s.into_std_path_buf()),
        license_file: package.license_file.clone().map(|f| f.into_std_path_buf()),
        workspace_root: metadata.workspace_root.into_std_path_buf(),
        build_dir: metadata
            .target_directory
            .into_std_path_buf()
            .join("wasm32-wasi"),
    })
}
