use cargo_metadata::{CargoOpt, MetadataCommand};
use clap::Parser;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

/// CLI args for the `wasmer init` command
#[derive(Debug, Parser)]
pub struct Init {
    /// Initialize wapm.toml for a library package
    #[clap(long, name = "lib")]
    pub lib: bool,
    /// Initialize wapm.toml for a binary package
    #[clap(long, name = "bin")]
    pub bin: bool,
    /// Initialize an empty wapm.toml
    #[clap(long, name = "empty")]
    pub empty: bool,
    /// If the `manifest-dir` contains a Cargo.toml, use that file to initialize the wapm.toml
    #[clap(long, name = "manifest-dir")]
    pub manifest_dir: Option<PathBuf>,
    /// Directory of the output file name. wasmer init will error in the target dir already contains a wapm.toml
    #[clap(long, short = 'o', name = "out")]
    pub out: Option<PathBuf>,
    /// Force overwriting the wapm.toml, even if it already exists
    #[clap(long, name = "overwrite")]
    pub overwrite: bool,
    /// Don't display debug output
    #[clap(long, name = "quiet")]
    pub quiet: bool,
    /// Add default dependencies for common packages (currently supported: `python`, `js`)
    #[clap(long, name = "template")]
    pub template: Option<String>,
    /// Include file paths into the target container filesystem
    #[clap(long, name = "include")]
    pub include: Vec<String>,
    /// Name of the package, defaults to the name of the init directory
    #[clap(name = "PACKAGE_NAME")]
    pub package_name: Option<String>,
}

#[derive(Debug, PartialEq, Copy, Clone)]
enum BinOrLib {
    Bin,
    Lib,
    Empty,
}

// minimal version of the Cargo.toml [package] section
struct MiniCargoTomlPackage {
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

impl Init {
    /// `wasmer init` execution
    pub fn execute(&self) -> Result<(), anyhow::Error> {
        let bin_or_lib = match (self.empty, self.bin, self.lib) {
            (true, true, _) | (true, _, true) => {
                return Err(anyhow::anyhow!(
                    "cannot combine --empty with --bin or --lib"
                ))
            }
            (true, false, false) => BinOrLib::Empty,
            (_, true, true) => {
                return Err(anyhow::anyhow!(
                    "cannot initialize a wapm manifest with both --bin and --lib, pick one"
                ))
            }
            (false, true, _) => BinOrLib::Bin,
            (false, _, true) => BinOrLib::Lib,
            _ => BinOrLib::Bin,
        };

        let target_file = match self.out.as_ref() {
            None => std::env::current_dir()?.join("wapm.toml"),
            Some(s) => {
                let _ = std::fs::create_dir_all(s);
                s.join("wapm.toml")
            }
        };

        if target_file.exists() && !self.overwrite {
            return Err(anyhow::anyhow!(
                "wapm project already initialized in {}",
                target_file.display()
            ));
        }

        // See if the directory has a Cargo.toml file, if yes, copy the license / readme, etc.
        let cargo_toml = MetadataCommand::new()
            .manifest_path(match self.manifest_dir.as_ref() {
                Some(s) => {
                    if format!("{}", s.display()).ends_with("Cargo.toml") {
                        s.clone()
                    } else {
                        s.join("Cargo.toml")
                    }
                }
                None => Path::new("./Cargo.toml").to_path_buf(),
            })
            .features(CargoOpt::AllFeatures)
            .exec()
            .ok();

        let cargo_toml = cargo_toml.and_then(|metadata| {
            let package = metadata.root_package()?;
            Some(MiniCargoTomlPackage {
                name: package.name.clone(),
                version: package.version.clone(),
                description: package.description.clone(),
                homepage: package.homepage.clone(),
                repository: package.repository.clone(),
                license: package.license.clone(),
                readme: package.readme.clone().map(|s| s.into_std_path_buf()),
                license_file: package.license_file.clone().map(|f| f.into_std_path_buf()),
                workspace_root: metadata.workspace_root.into_std_path_buf(),
                build_dir: metadata.target_directory.into_std_path_buf(),
            })
        });

        let package_name = match cargo_toml.as_ref() {
            None => {
                if let Some(parent) = target_file
                    .parent()
                    .and_then(|p| Some(p.file_stem()?.to_str()?.to_string()))
                {
                    parent
                } else {
                    std::env::current_dir()?
                        .file_stem()
                        .ok_or_else(|| anyhow!("current dir has no file stem"))?
                        .to_str()
                        .ok_or_else(|| anyhow!("current dir has no file stem"))?
                        .to_string()
                }
            }
            Some(s) => s.name.clone(),
        };

        let module_name = package_name.split('/').next().unwrap_or(&package_name);
        let version = cargo_toml
            .as_ref()
            .map(|t| t.version.clone())
            .unwrap_or_else(|| semver::Version::parse("0.1.0").unwrap());
        let license = cargo_toml.as_ref().and_then(|t| t.license.clone());
        let license_file = cargo_toml.as_ref().and_then(|t| t.license_file.clone());
        let readme = cargo_toml.as_ref().and_then(|t| t.readme.clone());
        let repository = cargo_toml.as_ref().and_then(|t| t.repository.clone());
        let homepage = cargo_toml.as_ref().and_then(|t| t.homepage.clone());
        let description = cargo_toml
            .as_ref()
            .and_then(|t| t.description.clone())
            .unwrap_or(format!("Description for package {module_name}"));

        let modules = vec![wapm_toml::Module {
            name: module_name.to_string(),
            source: cargo_toml
                .as_ref()
                .map(|p| {
                    p.build_dir
                        .join("release")
                        .join(&format!("{module_name}.wasm"))
                })
                .unwrap_or_else(|| Path::new(&format!("{module_name}.wasm")).to_path_buf()),
            kind: None,
            abi: wapm_toml::Abi::Wasi,
            bindings: match bin_or_lib {
                BinOrLib::Bin | BinOrLib::Empty => None,
                BinOrLib::Lib => target_file.parent().and_then(|parent| {
                    walkdir::WalkDir::new(parent)
                        .min_depth(1)
                        .max_depth(3)
                        .follow_links(false)
                        .into_iter()
                        .filter_map(|e| e.ok())
                        .filter_map(|e| {
                            let is_wit =
                                e.path().extension().and_then(|s| s.to_str()) == Some(".wit");
                            let is_wai =
                                e.path().extension().and_then(|s| s.to_str()) == Some(".wai");
                            if is_wit {
                                Some(wapm_toml::Bindings::Wit(wapm_toml::WitBindings {
                                    wit_exports: e.path().to_path_buf(),
                                    wit_bindgen: semver::Version::parse("0.1.0").unwrap(),
                                }))
                            } else if is_wai {
                                Some(wapm_toml::Bindings::Wai(wapm_toml::WaiBindings {
                                    exports: None,
                                    imports: vec![e.path().to_path_buf()],
                                    wai_version: semver::Version::parse("0.1.0").unwrap(),
                                }))
                            } else {
                                None
                            }
                        })
                        .next()
                }),
            },
            interfaces: Some({
                let mut map = HashMap::new();
                map.insert("wasi".to_string(), "0.1.0-unstable".to_string());
                map
            }),
        }];

        let default_manifest = wapm_toml::Manifest {
            package: wapm_toml::Package {
                name: package_name.clone(),
                version,
                description,
                license,
                license_file,
                readme,
                repository,
                homepage,
                wasmer_extra_flags: None,
                disable_command_rename: false,
                rename_commands_to_raw_command_name: false,
            },
            dependencies: Some({
                match self.template.as_deref() {
                    Some("js") => {
                        let mut map = HashMap::default();
                        map.insert("python".to_string(), "quickjs/quickjs@latest".to_string());
                        map
                    }
                    Some("python") => {
                        let mut map = HashMap::default();
                        map.insert("python".to_string(), "python/python@latest".to_string());
                        map
                    }
                    _ => HashMap::default(),
                }
            }),
            command: match bin_or_lib {
                BinOrLib::Bin => Some(
                    modules
                        .iter()
                        .map(|m| {
                            wapm_toml::Command::V1(wapm_toml::CommandV1 {
                                name: m.name.clone(),
                                module: m.name.clone(),
                                main_args: None,
                                package: None,
                            })
                        })
                        .collect(),
                ),
                BinOrLib::Lib | BinOrLib::Empty => None,
            },
            module: match bin_or_lib {
                BinOrLib::Empty => None,
                _ => Some(modules),
            },
            fs: if self.include.is_empty() {
                None
            } else {
                Some(
                    self.include
                        .iter()
                        .map(|path| {
                            if path == "." || path == "/" {
                                ("/".to_string(), Path::new("/").to_path_buf())
                            } else {
                                (
                                    format!("./{path}"),
                                    Path::new(&format!("/{path}")).to_path_buf(),
                                )
                            }
                        })
                        .collect(),
                )
            },
            base_directory_path: target_file
                .parent()
                .map(|o| o.to_path_buf())
                .unwrap_or_else(|| target_file.clone()),
        };

        if let Some(parent) = target_file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        let cargo_wapm_stdout = std::process::Command::new("cargo")
            .arg("wapm")
            .arg("--version")
            .output()
            .map(|s| String::from_utf8_lossy(&s.stdout).to_string())
            .unwrap_or_default();

        let cargo_wapm_present =
            cargo_wapm_stdout.lines().count() == 1 && cargo_wapm_stdout.contains("cargo wapm");
        let should_add_to_cargo_toml = cargo_toml.is_some() && cargo_wapm_present;

        if !cargo_wapm_present && cargo_toml.is_some() && !self.quiet {
            eprintln!(
                "Note: you seem to have a Cargo.toml file, but you haven't installed `cargo wapm`."
            );
            eprintln!("You can build and release Rust projects directly with `cargo wapm publish`: https://crates.io/crates/cargo-wapm");
            eprintln!("Install it with:");
            eprintln!();
            eprintln!("    cargo install cargo-wapm");
            eprintln!();
        }

        if should_add_to_cargo_toml {
            let toml_string = toml::to_string_pretty(&default_manifest)?;
            let mut value = toml_string.parse::<toml::Value>().unwrap();
            value.as_table_mut().unwrap().remove("package");

            let mut new_table = toml::value::Table::new();
            new_table.insert("wapm".to_string(), value);

            let mut table_2 = toml::value::Table::new();
            table_2.insert("metadata".to_string(), new_table.into());

            let toml_string = toml::to_string_pretty(&table_2)?;

            let cargo_toml_path = target_file.parent().unwrap().join("Cargo.toml");

            let old_cargo = std::fs::read_to_string(&cargo_toml_path).unwrap();

            if old_cargo.contains("metadata.wapm") && !self.overwrite {
                return Err(anyhow::anyhow!(
                    "wapm project already initialized in Cargo.toml file"
                ));
            } else {
                if !self.quiet {
                    eprintln!("You have cargo-wapm installed, added metadata to Cargo.toml instead of wapm.toml");
                    eprintln!("Build and publish your package with:");
                    eprintln!();
                    eprintln!("    cargo wapm publish");
                    eprintln!();
                }
                std::fs::write(
                    &cargo_toml_path,
                    &format!("{old_cargo}\r\n\r\n# See more keys and definitions at https://docs.wasmer.io/ecosystem/wapm/manifest\r\n\r\n{toml_string}"),
                )?;
            }
        } else {
            let toml_string = toml::to_string_pretty(&default_manifest)?
            .replace("[dependencies]", "# See more keys and definitions at https://docs.wasmer.io/ecosystem/wapm/manifest\r\n\r\n[dependencies]")
            .lines()
            .collect::<Vec<_>>()
            .join("\r\n");

            std::fs::write(target_file, &toml_string)?;
        }

        Ok(())
    }
}
