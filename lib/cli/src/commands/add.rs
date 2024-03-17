use std::process::{Command, Stdio};

use anyhow::{Context, Error};
use clap::Parser;
use wasmer_api::{
    types::{PackageVersionLanguageBinding, ProgrammingLanguage},
    WasmerClient,
};

use crate::opts::ApiOpts;

use super::AsyncCliCommand;

/// Add a Wasmer package's bindings to your application.
#[derive(Debug, Parser)]
pub struct Add {
    #[clap(flatten)]
    #[allow(missing_docs)]
    pub api: ApiOpts,

    /// Add the JavaScript bindings using "npm install".
    #[clap(long, groups = &["bindings", "js"])]
    npm: bool,
    /// Add the JavaScript bindings using "yarn add".
    #[clap(long, groups = &["bindings", "js"])]
    yarn: bool,
    /// Add the JavaScript bindings using "pnpm add".
    #[clap(long, groups = &["bindings", "js"])]
    pnpm: bool,
    /// Add the package as a dev-dependency.
    #[clap(long, requires = "js")]
    dev: bool,
    /// Add the Python bindings using "pip install".
    #[clap(long, groups = &["bindings", "py"])]
    pip: bool,
    /// The packages to add (e.g. "wasmer/wasmer-pack@0.5.0" or "python/python")
    packages: Vec<wasmer_registry::Package>,
}

#[async_trait::async_trait]
impl AsyncCliCommand for Add {
    type Output = ();

    async fn run_async(self) -> Result<Self::Output, anyhow::Error> {
        anyhow::ensure!(!self.packages.is_empty(), "No packages specified");

        let client = self.api.client()?;

        let bindings = self.lookup_bindings(&client).await?;

        let mut cmd = self.target()?.command(&bindings)?;
        cmd.stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());

        println!("Running: {cmd:?}");

        let status = cmd.status().with_context(|| {
            format!(
                "Unable to start \"{:?}\". Is it installed?",
                cmd.get_program()
            )
        })?;

        anyhow::ensure!(status.success(), "Command failed: {:?}", cmd);

        Ok(())
    }
}

impl Add {
    async fn lookup_bindings(
        &self,
        client: &WasmerClient,
    ) -> Result<Vec<PackageVersionLanguageBinding>, Error> {
        println!("Querying Wasmer for package bindings");

        let mut bindings = Vec::new();
        let language = self.target()?.language();

        for pkg in &self.packages {
            let binding = lookup_bindings_for_package(client, pkg, &language).await?;
            bindings.push(binding);
        }

        Ok(bindings)
    }

    fn target(&self) -> Result<Target, Error> {
        match (self.pip, self.npm, self.yarn, self.pnpm) {
            (false, false, false, false) => Err(anyhow::anyhow!(
                "at least one of --npm, --pip, --yarn or --pnpm has to be specified"
            )),
            (true, false, false, false) => Ok(Target::Pip),
            (false, true, false, false) => Ok(Target::Npm { dev: self.dev }),
            (false, false, true, false) => Ok(Target::Yarn { dev: self.dev }),
            (false, false, false, true) => Ok(Target::Pnpm { dev: self.dev }),
            _ => Err(anyhow::anyhow!(
                "only one of --npm, --pip or --yarn has to be specified"
            )),
        }
    }
}

async fn lookup_bindings_for_package(
    client: &WasmerClient,
    pkg: &wasmer_registry::Package,
    language: &ProgrammingLanguage,
) -> Result<PackageVersionLanguageBinding, Error> {
    let full_name = format!("{}/{}", pkg.namespace, pkg.name);
    let all_bindings =
        wasmer_api::query::get_package_version_bindings(client, full_name, pkg.version.clone())
            .await?;

    all_bindings
        .into_iter()
        .find(|b| b.language == *language)
        .with_context(|| format!("{pkg} does not contain bindings for {}", language.as_str()))
}

#[derive(Debug, Copy, Clone)]
enum Target {
    Pip,
    Yarn { dev: bool },
    Npm { dev: bool },
    Pnpm { dev: bool },
}

impl Target {
    fn language(self) -> ProgrammingLanguage {
        match self {
            Target::Pip => ProgrammingLanguage::Python,
            Target::Pnpm { .. } | Target::Yarn { .. } | Target::Npm { .. } => {
                ProgrammingLanguage::Javascript
            }
        }
    }

    /// Construct a command which we can run to add packages.
    ///
    /// This deliberately runs the command using the OS shell instead of
    /// invoking the tool directly. That way we can handle when a version
    /// manager (e.g. `nvm` or `asdf`) replaces the tool with a script (e.g.
    /// `npm.cmd` or `yarn.ps1`).
    ///
    /// See <https://github.com/wasmerio/wapm-cli/issues/291> for more.
    fn command(self, packages: &[PackageVersionLanguageBinding]) -> Result<Command, Error> {
        let command_line = match self {
            Target::Pip => {
                if Command::new("pip").arg("--version").output().is_ok() {
                    "pip install"
                } else if Command::new("pip3").arg("--version").output().is_ok() {
                    "pip3 install"
                } else if Command::new("python").arg("--version").output().is_ok() {
                    "python -m pip install"
                } else if Command::new("python3").arg("--version").output().is_ok() {
                    "python3 -m pip install"
                } else {
                    return Err(anyhow::anyhow!(
                        "neither pip, pip3, python or python3 installed"
                    ));
                }
            }
            Target::Yarn { dev } => {
                if Command::new("yarn").arg("--version").output().is_err() {
                    return Err(anyhow::anyhow!("yarn not installed"));
                }
                if dev {
                    "yarn add --dev"
                } else {
                    "yarn add"
                }
            }
            Target::Npm { dev } => {
                if Command::new("npm").arg("--version").output().is_err() {
                    return Err(anyhow::anyhow!("npm not installed"));
                }
                if dev {
                    "npm install --dev"
                } else {
                    "npm install"
                }
            }
            Target::Pnpm { dev } => {
                if Command::new("pnpm").arg("--version").output().is_err() {
                    return Err(anyhow::anyhow!("pnpm not installed"));
                }
                if dev {
                    "pnpm add --dev"
                } else {
                    "pnpm add"
                }
            }
        };
        let mut command_line = command_line.to_string();

        for pkg in packages {
            command_line.push(' ');
            command_line.push_str(&pkg.url);
        }

        if cfg!(windows) {
            let mut cmd = Command::new("cmd");
            cmd.arg("/C").arg(command_line);
            Ok(cmd)
        } else {
            let mut cmd = Command::new("sh");
            cmd.arg("-c").arg(command_line);
            Ok(cmd)
        }
    }
}
