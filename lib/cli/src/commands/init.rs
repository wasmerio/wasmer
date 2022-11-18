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
    /// Path to the directory to init the wapm.toml in
    #[clap(long, name = "dir", env = "DIR", parse(from_os_str))]
    pub dir: Option<PathBuf>,
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

impl Init {
    /// `wasmer init` execution
    pub fn execute(&self) -> Result<(), anyhow::Error> {
        let target_file = match self.dir.as_ref() {
            None => std::env::current_dir()?.join("wapm.toml"),
            Some(s) => {
                let _ = std::fs::create_dir_all(s);
                s.join("wapm.toml")
            }
        };

        if target_file.exists() {
            return Err(anyhow::anyhow!(
                "wapm project already initialized in {}",
                target_file.display()
            ));
        }

        let package_name = match self.package_name.as_ref() {
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
            Some(s) => s.to_string(),
        };

        // See if the directory has a Cargo.toml file, if yes, copy the license / readme, etc.
        let cargo_toml = target_file.parent().and_then(|p| {
            let file = std::fs::read_to_string(p.join("Cargo.toml")).ok()?;
            file.parse::<toml::Value>().ok()
        });
        let version = cargo_toml
            .as_ref()
            .and_then(|toml| {
                semver::Version::parse(toml.get("package")?.as_table()?.get("version")?.as_str()?)
                    .ok()
            })
            .unwrap_or_else(|| semver::Version::parse("0.1.0").unwrap());

        let license = cargo_toml.as_ref().and_then(|toml| {
            Some(
                toml.get("package")?
                    .as_table()?
                    .get("license")?
                    .as_str()?
                    .to_string(),
            )
        });

        let license_file = cargo_toml.as_ref().and_then(|toml| {
            Some(
                Path::new(
                    toml.get("package")?
                        .as_table()?
                        .get("license_file")?
                        .as_str()?,
                )
                .to_path_buf(),
            )
        });

        let readme = cargo_toml.as_ref().and_then(|toml| {
            Some(Path::new(toml.get("package")?.as_table()?.get("readme")?.as_str()?).to_path_buf())
        });

        let repository = cargo_toml.as_ref().and_then(|toml| {
            Some(
                toml.get("package")?
                    .as_table()?
                    .get("repository")?
                    .as_str()?
                    .to_string(),
            )
        });

        let homepage = cargo_toml.as_ref().and_then(|toml| {
            Some(
                toml.get("package")?
                    .as_table()?
                    .get("homepage")?
                    .as_str()?
                    .to_string(),
            )
        });

        let description = cargo_toml
            .as_ref()
            .and_then(|toml| {
                Some(
                    toml.get("package")?
                        .as_table()?
                        .get("description")?
                        .as_str()?
                        .to_string(),
                )
            })
            .unwrap_or_else(|| format!("Description of the package {package_name}"));

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

        let module_name = package_name.split('/').next().unwrap_or(&package_name);

        let modules = vec![wapm_toml::Module {
            name: module_name.to_string(),
            source: if cargo_toml.is_some() {
                Path::new(&format!("target/wasm32-wasi/release/{module_name}.wasm")).to_path_buf()
            } else {
                Path::new(&format!("{module_name}.wasm")).to_path_buf()
            },
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
                        .filter(|e| {
                            e.path().extension().and_then(|s| s.to_str()) == Some(".wit")
                                || e.path().extension().and_then(|s| s.to_str()) == Some(".wai")
                        })
                        .map(|e| wapm_toml::Bindings {
                            wit_exports: e.path().to_path_buf(),
                            wit_bindgen: semver::Version::parse("0.1.0").unwrap(),
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

        if !cargo_wapm_present && cargo_toml.is_some() {
            eprintln!(
                "Note: you seem to have a Cargo.toml file, but you haven't installed `cargo wapm`."
            );
            eprintln!("You can build and release Rust projects directly with `cargo wapm publish`: https://crates.io/crates/cargo-wapm");
            eprintln!("Install it with:");
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

            if old_cargo.contains("metadata.wapm") {
                return Err(anyhow::anyhow!(
                    "wapm project already initialized in Cargo.toml file"
                ));
            } else {
                eprintln!("You have cargo-wapm installed, added metadata to Cargo.toml instead of wapm.toml");
                eprintln!("Build and publish your package with:");
                eprintln!();
                eprintln!("    cargo wapm publish");
                eprintln!();
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
