use std::path::Path;

use anyhow::{Context, bail};
use dialoguer::console::style;
use wasmer_config::package::{Manifest, PackageIdent, PackageSource};

use super::common::{get_manifest, manifest_from_webc_metadata, package_web_url};
use crate::commands::AsyncCliCommand;
use crate::config::WasmerEnv;

/// Show basic metadata of a package without unpacking or downloading it.
///
/// The package can be a directory containing a `wasmer.toml` file, a `.webc`
/// file, or a package name from the registry. If no argument is given, the
/// current directory is used.
#[derive(clap::Parser, Debug)]
pub struct PackageGet {
    #[clap(flatten)]
    pub env: WasmerEnv,

    /// The package to show.
    ///
    /// This can be a path to a package directory or a `.webc` file, or the name
    /// of a package in the registry (e.g. `wasmer/hello@=0.1.0`).
    #[clap(default_value = ".")]
    pub package: String,
}

#[async_trait::async_trait]
impl AsyncCliCommand for PackageGet {
    type Output = ();

    async fn run_async(self) -> Result<Self::Output, anyhow::Error> {
        let (manifest, web_url) = self.load_manifest().await?;

        let print_field = |label: &str, value: &str| {
            println!("{:<13} {value}", style(format!("{label}:")).bold().dim());
        };

        if let Some(package) = manifest.package.as_ref() {
            print_field("Name", package.name.as_deref().unwrap_or("<unnamed>"));
            print_field(
                "Version",
                &package
                    .version
                    .as_ref()
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "<none>".to_string()),
            );

            if let Some(description) = &package.description {
                print_field("Description", description);
            }
            if let Some(license) = &package.license {
                print_field("License", license);
            }
            if let Some(homepage) = &package.homepage {
                print_field("Homepage", homepage);
            }
            if let Some(repository) = &package.repository {
                print_field("Repository", repository);
            }
            if let Some(entrypoint) = &package.entrypoint {
                print_field("Entrypoint", entrypoint);
            }
            if package.private {
                print_field("Private", "true");
            }
        } else {
            println!("{}", style("Package has no metadata.").dim());
        }

        if !manifest.commands.is_empty() {
            let commands = manifest
                .commands
                .iter()
                .map(|c| c.get_name())
                .collect::<Vec<_>>()
                .join(", ");
            print_field("Commands", &commands);
        }

        if !manifest.dependencies.is_empty() {
            print_field("Dependencies", &manifest.dependencies.len().to_string());
            for (name, version) in &manifest.dependencies {
                println!("  {name} = {version}");
            }
        }

        if let Some(web_url) = web_url {
            print_field("URL", &web_url);
        }

        Ok(())
    }
}

impl PackageGet {
    /// Resolve the `package` argument into a [`Manifest`], whether it points to a
    /// local package or a package in the registry.
    ///
    /// The second tuple element is a link to the package's page on the registry
    /// web frontend, present only when the package was resolved from a named
    /// registry lookup (local files and hashes have no such page).
    async fn load_manifest(&self) -> anyhow::Result<(Manifest, Option<String>)> {
        // A path on disk (directory with a `wasmer.toml`, or a `.webc` file)
        // takes precedence over registry resolution.
        let path = Path::new(&self.package);
        if path.exists() {
            let (_, manifest) = get_manifest(path)?;
            return Ok((manifest, None));
        }

        let source: PackageSource = self.package.parse().with_context(|| {
            format!(
                "'{}' is not a file or directory on disk, or a valid package name",
                self.package
            )
        })?;

        match source {
            PackageSource::Ident(PackageIdent::Named(id)) => {
                let client = self.env.client_unauthennticated()?;

                let version = id.version_or_default().to_string();
                let version = if version == "*" {
                    String::from("latest")
                } else {
                    version
                };
                let full_name = id.full_name();

                let package = wasmer_backend_api::query::get_package_version(
                    &client,
                    full_name.clone(),
                    version.clone(),
                )
                .await?
                .with_context(|| {
                    format!(
                        "could not retrieve package information for package '{}' from registry '{}'",
                        full_name,
                        client.graphql_endpoint(),
                    )
                })?;

                let json = package
                    .pirita_manifest
                    .as_ref()
                    .context("the registry did not return a manifest for this package")?;
                let webc_manifest: webc::metadata::Manifest = serde_json::from_str(&json.0)
                    .context("could not parse the manifest returned by the registry")?;
                let mut manifest = manifest_from_webc_metadata(&webc_manifest)?;

                // Link to the package's page on the registry web frontend,
                // using the concrete version the registry resolved for us.
                let web_url = package_web_url(&client, &full_name, Some(&package.version));

                // Prefer the authoritative metadata reported by the registry.
                //
                // `Package::from_manifest` strips name/version/description from
                // the WAPM annotation when building the webc, so the manifest
                // reconstructed from `pirita_manifest` is missing them. The
                // registry exposes these fields directly, so re-inject them.
                if let Some(pkg) = manifest.package.as_mut() {
                    pkg.name = Some(full_name);
                    pkg.version = Some(package.version.parse().with_context(|| {
                        format!(
                            "invalid version returned by the registry: '{}'",
                            package.version
                        )
                    })?);
                    // Take each registry field when present, otherwise keep
                    // whatever the webc carried (license/homepage/repository
                    // survive in the WAPM annotation; description does not).
                    if !package.description.is_empty() {
                        pkg.description = Some(package.description);
                    }
                    if package.license.is_some() {
                        pkg.license = package.license;
                    }
                    if package.homepage.is_some() {
                        pkg.homepage = package.homepage;
                    }
                    if package.repository.is_some() {
                        pkg.repository = package.repository;
                    }
                }

                Ok((manifest, Some(web_url)))
            }
            PackageSource::Ident(PackageIdent::Hash(hash)) => {
                let client = self.env.client_unauthennticated()?;

                let pkg = wasmer_backend_api::query::get_package_release(
                    &client,
                    &hash.to_string(),
                )
                .await?
                .with_context(|| {
                    format!(
                        "Package with {hash} does not exist in the registry, or is not accessible"
                    )
                })?;

                let image = pkg
                    .webc_v3
                    .or(pkg.webc)
                    .context("the registry did not return a WebC image for this package")?;
                let webc_manifest: webc::metadata::Manifest =
                    serde_json::from_str(&image.manifest.0)
                        .context("could not parse the manifest returned by the registry")?;

                Ok((manifest_from_webc_metadata(&webc_manifest)?, None))
            }
            PackageSource::Path(p) => {
                // Local paths will have already short-circuited at the very
                // start of the command, so we should never get here,
                // unless the local path is invalid or inaccessible.
                bail!("no file or directory found at '{p}'")
            }
            PackageSource::Url(url) => {
                bail!("showing a package directly from a URL is not supported: '{url}'")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cmd_package_get_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("wasmer.toml"),
            r#"
[package]
name = "wasmer/test"
version = "1.2.3"
description = "A test package"
license = "MIT"
"#,
        )
        .unwrap();

        let cmd = PackageGet {
            env: WasmerEnv::new(
                crate::config::DEFAULT_WASMER_CACHE_DIR.clone(),
                crate::config::DEFAULT_WASMER_CACHE_DIR.clone(),
                None,
                None,
            ),
            package: dir.path().to_str().unwrap().to_owned(),
        };

        cmd.run_async().await.unwrap();
    }
}
