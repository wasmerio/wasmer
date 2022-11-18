use clap::Parser;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

/// CLI args for the `wasmer init` command
#[derive(Debug, Parser)]
pub struct Init {
    /// Name of the package, defaults to the name of the init directory
    #[clap(name = "PACKAGE_NAME")]
    pub package_name: Option<String>,
    /// Path to the directory to init the wapm.toml in
    #[clap(long, name = "dir", env = "DIR", parse(from_os_str))]
    pub dir: Option<PathBuf>,
    /// Initialize wapm.toml for a library package
    #[clap(long, name = "lib")]
    pub lib: Option<bool>,
    /// Initialize wapm.toml for a binary package
    #[clap(long, name = "bin")]
    pub bin: Option<bool>,
    /// Add default dependencies for common packages (currently supported: `python`, `js`)
    #[clap(long, name = "template")]
    pub template: Option<String>,
    /// Include file paths into the target container filesystem
    #[clap(long)]
    #[clap(long, name = "include")]
    pub include: Vec<PathBuf>,
}

#[derive(PartialEq, Copy, Clone)]
enum BinOrLib {
    Bin,
    Lib,
}

impl Init {
    /// `wasmer init` execution
    pub fn execute(&self) -> Result<(), anyhow::Error> {
        let package_name = match self.package_name.as_ref() {
            None => std::env::current_dir()?
                .file_stem()
                .ok_or_else(|| anyhow!("current dir has no file stem"))?
                .to_str()
                .ok_or_else(|| anyhow!("current dir has no file stem"))?
                .to_string(),
            Some(s) => s.to_string(),
        };

        let target_file = match self.dir.as_ref() {
            None => std::env::current_dir()?.join("wapm.toml"),
            Some(s) => {
                if !s.exists() {
                    let _ = std::fs::create_dir_all(s);
                }
                s.join("wapm.toml")
            }
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
            .unwrap_or(semver::Version::parse("0.1.0").unwrap());

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

        let bin_or_lib = match (self.bin, self.lib) {
            (Some(true), Some(true)) => {
                return Err(anyhow::anyhow!(
                    "cannot initialize a wapm manifest with both --bin and --lib, pick one"
                ))
            }
            (Some(true), _) => BinOrLib::Bin,
            (_, Some(true)) => BinOrLib::Lib,
            _ => BinOrLib::Bin,
        };

        let module_name = package_name.split("/").next().unwrap_or(&package_name);

        let modules = vec![wapm_toml::Module {
            name: module_name.to_string(),
            source: if cargo_toml.is_some() {
                Path::new(&format!("target/release/wasm32-wasi/{module_name}.wasm")).to_path_buf()
            } else {
                Path::new(&format!("{module_name}.wasm")).to_path_buf()
            },
            kind: None,
            abi: wapm_toml::Abi::Wasi,
            bindings: match bin_or_lib {
                BinOrLib::Bin => None,
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
                version: version,
                description,
                license: license,
                license_file: license_file,
                readme: readme,
                repository: repository,
                homepage: homepage,
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
                BinOrLib::Lib => None,
            },
            module: Some(modules),
            fs: if self.include.is_empty() {
                None
            } else {
                Some(
                    self.include
                        .iter()
                        .filter_map(|path| {
                            let path = Path::new(path);
                            if !path.exists() {
                                return None;
                            }
                            Some((
                                format!("{}", path.display()),
                                Path::new(&format!("/{}", path.display())).to_path_buf(),
                            ))
                        })
                        .collect(),
                )
            },
            base_directory_path: target_file
                .parent()
                .map(|o| o.to_path_buf())
                .unwrap_or(target_file.clone()),
        };

        println!(
            "package: {package_name:?} target: {} - {:?}, wapm_toml = {}",
            target_file.display(),
            self,
            toml::to_string_pretty(&default_manifest).unwrap_or_default(),
        );

        Ok(())
    }
}
