use crate::{
    commands::{
        package::common::{macros::*, wait::wait_package, *},
        AsyncCliCommand,
    },
    config::WasmerEnv,
};
use anyhow::Context;
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Confirm};
use is_terminal::IsTerminal;
use std::{
    path::{Path, PathBuf},
    str::FromStr,
};
use wasmer_backend_api::WasmerClient;
use wasmer_config::package::{
    Manifest, NamedPackageId, NamedPackageIdent, PackageBuilder, PackageHash, PackageIdent,
};

use super::PublishWait;

/// Tag an existing package.
#[derive(Debug, clap::Parser)]
pub struct PackageTag {
    #[clap(flatten)]
    pub env: WasmerEnv,

    /// Run the publish logic without sending anything to the registry server
    #[clap(long, name = "dry-run")]
    pub dry_run: bool,

    /// Run the publish command without any output
    #[clap(long)]
    pub quiet: bool,

    /// Override the namespace of the package to upload
    #[clap(long = "namespace")]
    pub package_namespace: Option<String>,

    /// Override the name of the package to upload
    #[clap(long = "name")]
    pub package_name: Option<String>,

    /// Override the package version of the uploaded package in the wasmer.toml
    #[clap(long = "version")]
    pub package_version: Option<semver::Version>,

    /// Timeout (in seconds) for the publish query to the registry.
    ///
    /// Note that this is not the timeout for the entire publish process, but
    /// for each individual query to the registry during the publish flow.
    #[clap(long, default_value = "5m")]
    pub timeout: humantime::Duration,

    /// Whether or not the patch field of the version of the package - if any - should be bumped.
    #[clap(long, conflicts_with = "package_version")]
    pub bump: bool,

    /// Do not prompt for user input.
    #[clap(long, default_value_t = !std::io::stdin().is_terminal())]
    pub non_interactive: bool,

    /// The hash of the package to tag
    #[clap(name = "hash")]
    pub package_hash: PackageHash,

    /// The package to tag.
    #[clap(name = "package_ident")]
    pub package_id: Option<NamedPackageIdent>,

    /// Directory containing the `wasmer.toml`, or a custom *.toml manifest file.
    ///
    /// Defaults to current working directory.
    #[clap(long = "path", default_value = ".")]
    pub package_path: PathBuf,

    /// Wait for package to be available on the registry before exiting.
    #[clap(
            long,
            require_equals = true,
            num_args = 0..=1,
            default_value_t = PublishWait::None,
            default_missing_value = "container",
            value_enum
        )]
    pub wait: PublishWait,
}

impl PackageTag {
    async fn update_manifest_name(
        &self,
        manifest_path: Option<&Path>,
        manifest: Option<&Manifest>,
        full_name: &str,
    ) -> anyhow::Result<Option<Manifest>> {
        if manifest_path.is_none() && manifest.is_none() {
            return Ok(None);
        }

        let mut new_manifest = manifest.cloned().unwrap();
        let manifest_path = manifest_path.unwrap();

        if let Some(pkg) = &mut new_manifest.package {
            pkg.name = Some(full_name.to_string());
        } else {
            let package = PackageBuilder::default().name(full_name).build()?;
            new_manifest.package = Some(package);
        }

        let manifest_raw = toml::to_string(&new_manifest)?;

        tokio::fs::write(manifest_path, manifest_raw)
            .await
            .context("while trying to serialize the manifest")?;

        Ok(Some(new_manifest))
    }

    async fn update_manifest_version(
        &self,
        manifest_path: Option<&Path>,
        manifest: Option<&Manifest>,
        user_version: &semver::Version,
    ) -> anyhow::Result<Option<Manifest>> {
        if manifest_path.is_none() && manifest.is_none() {
            return Ok(None);
        }

        let mut new_manifest = manifest.cloned().unwrap();
        let manifest_path = manifest_path.unwrap();

        if let Some(pkg) = &mut new_manifest.package {
            pkg.version = Some(user_version.clone());
        } else {
            let package = PackageBuilder::default()
                .version(user_version.clone())
                .build()?;
            new_manifest.package = Some(package);
        }

        let manifest_raw = toml::to_string(&new_manifest)?;

        tokio::fs::write(manifest_path, manifest_raw)
            .await
            .context("while trying to serialize the manifest")?;

        Ok(Some(new_manifest))
    }

    #[tracing::instrument]
    async fn do_tag(
        &self,
        client: &WasmerClient,
        id: &NamedPackageId,
        manifest: Option<&Manifest>,
        package_release_id: &wasmer_backend_api::types::Id,
    ) -> anyhow::Result<()> {
        tracing::info!(
            "Tagging package with registry id {:?} and specifier {:?}",
            package_release_id,
            id
        );

        let pb = make_spinner!(self.quiet, "Tagging package...");

        let NamedPackageId { full_name, version } = id;
        let maybe_description = manifest
            .and_then(|m| m.package.as_ref())
            .and_then(|p| p.description.clone());
        let maybe_homepage = manifest
            .and_then(|m| m.package.as_ref())
            .and_then(|p| p.homepage.clone());
        let maybe_license = manifest
            .and_then(|m| m.package.as_ref())
            .and_then(|p| p.license.clone());
        let maybe_license_file = manifest
            .and_then(|m| m.package.as_ref())
            .and_then(|p| p.license_file.clone())
            .map(|f| f.to_string_lossy().to_string());
        let maybe_readme = manifest
            .and_then(|m| m.package.as_ref())
            .and_then(|p| p.readme.clone())
            .map(|f| f.to_string_lossy().to_string());
        let maybe_repository = manifest
            .and_then(|m| m.package.as_ref())
            .and_then(|p| p.repository.clone());

        let private = if let Some(pkg) = &manifest.and_then(|m| m.package.as_ref()) {
            Some(pkg.private)
        } else {
            Some(false)
        };

        let version = version.to_string();

        let manifest_raw = if let Some(manifest) = manifest {
            Some(toml::to_string(&manifest)?)
        } else {
            None
        };

        let maybe_readme_content = match maybe_readme {
            Some(readme) => {
                let readme_path = self.package_path.join(&readme);
                if readme_path.exists() {
                    Some(tokio::fs::read_to_string(readme_path).await?)
                } else {
                    None
                }
            }
            None => None,
        };

        let r = wasmer_backend_api::query::tag_package_release(
            client,
            maybe_description.as_deref(),
            maybe_homepage.as_deref(),
            maybe_license.as_deref(),
            maybe_license_file.as_deref(),
            manifest_raw.as_deref(),
            full_name,
            None,
            package_release_id,
            private,
            maybe_readme_content.as_deref(),
            maybe_repository.as_deref(),
            &version,
        );

        match r.await? {
            Some(r) => {
                if r.success {
                    spinner_ok!(pb, format!("Successfully tagged package {id}"));
                    if let Some(package_version) = r.package_version {
                        wait_package(client, self.wait, package_version.id, self.timeout).await?;
                    }
                    Ok(())
                } else {
                    spinner_err!(pb, "Could not tag package!");
                    anyhow::bail!("An unknown error occurred and the tagging failed.")
                }
            }
            None => {
                spinner_err!(pb, "Could not tag package!");
                anyhow::bail!("The registry returned an empty response.")
            }
        }
    }

    async fn get_package_id(
        &self,
        client: &WasmerClient,
        hash: &PackageHash,
        check_package_exists: bool,
    ) -> anyhow::Result<wasmer_backend_api::types::Id> {
        let pb = make_spinner!(
            self.quiet || check_package_exists,
            "Checking if the package exists.."
        );

        tracing::debug!("Searching for package with hash: {hash}");

        let pkg = match wasmer_backend_api::query::get_package_release(client, &hash.to_string())
            .await?
        {
            Some(p) => p,
            None => {
                spinner_err!(pb, "The package is not in the registry!");
                if !self.quiet {
                    eprintln!("\n\nThe package with the required hash does not exist in the selected registry.");
                    let bin_name = bin_name!();
                    let cli = std::env::args()
                        .filter(|s| !s.starts_with('-'))
                        .collect::<Vec<String>>()
                        .join(" ");

                    if cli.contains("publish") && self.dry_run {
                        eprintln!(
                            "{}: you are running `{cli}` with `--dry-run` set.\n",
                            "HINT".bold()
                        );
                    } else {
                        eprintln!(
                            "To first push the package to the registry, run `{}`.",
                            format!("{bin_name} package push").bold()
                        );
                        eprintln!(
                            "{}: you can also use `{}` to push {} tag your package.\n",
                            "NOTE".bold(),
                            format!("{bin_name} package publish").bold(),
                            "and".italic()
                        );
                    }
                }
                anyhow::bail!("Can't tag, no matching package found in the registry.")
            }
        };

        spinner_ok!(
            pb,
            format!(
                "Found package in the registry ({})",
                hash.to_string()
                    .trim_start_matches("sha256:")
                    .chars()
                    .take(7)
                    .collect::<String>()
            )
        );

        Ok(pkg.id)
    }

    fn get_name(
        &self,
        manifest: Option<&Manifest>,
        allow_unnamed: bool,
    ) -> anyhow::Result<Option<String>> {
        if let Some(name) = &self.package_id.as_ref().map(|id| id.name.clone()) {
            return Ok(Some(name.clone()));
        }

        // REMOVE ME: This is here for backwards compatibility, but we should remove the flag.
        if let Some(name) = &self.package_name {
            return Ok(Some(name.clone()));
        }

        if let Some(pkg) = &manifest.and_then(|m| m.package.as_ref()) {
            if let Some(ns) = &pkg.name {
                if let Some(name) = ns.split('/').nth(1) {
                    return Ok(Some(name.to_string()));
                }
            }
        }

        if allow_unnamed {
            return Ok(None);
        }

        if self.non_interactive {
            // if not interactive we can't prompt the user to choose the owner of the app.
            anyhow::bail!("No package name specified: use --name <package_name>");
        }

        let default_name = std::env::current_dir().ok().and_then(|dir| {
            dir.file_name()
                .and_then(|f| f.to_str())
                .map(|s| s.to_owned())
        });

        crate::utils::prompts::prompt_for_ident("Choose a package name", default_name.as_deref())
            .map(Some)
    }

    async fn get_namespace(
        &self,
        client: &WasmerClient,
        manifest: Option<&Manifest>,
    ) -> anyhow::Result<String> {
        if let Some(namespace) = self.package_id.as_ref().and_then(|id| id.namespace.clone()) {
            return Ok(namespace);
        }

        // REMOVE ME: This is here for backwards compatibility, but we should remove the flag.
        if let Some(namespace) = &self.package_namespace {
            return Ok(namespace.clone());
        }

        if let Some(pkg) = manifest.and_then(|m| m.package.clone()) {
            if let Some(name) = &pkg.name {
                if let Some(ns) = name.split('/').next() {
                    return Ok(ns.to_string());
                }
            }
        }

        if self.non_interactive {
            // if not interactive we can't prompt the user to choose the owner of the app.
            anyhow::bail!("No package namespace specified: use --namespace <package_namespace>");
        }

        let user = wasmer_backend_api::query::current_user_with_namespaces(client, None).await?;
        crate::utils::prompts::prompt_for_namespace("Choose a namespace", None, Some(&user))
    }

    async fn get_version(
        &mut self,
        client: &WasmerClient,
        manifest: Option<&Manifest>,
        manifest_path: Option<&Path>,
        full_pkg_name: &str,
    ) -> anyhow::Result<semver::Version> {
        if let Some(wasmer_config::package::Tag::VersionReq(r)) =
            self.package_id.as_ref().and_then(|id| id.tag.clone())
        {
            let mut version = r.to_string();
            version.remove(0);
            let version = semver::Version::parse(&version)?;
            return Ok(version);
        }

        // REMOVE ME: This is here for backwards compatibility, but we should remove the flag.
        if let Some(version) = &self.package_version {
            // If a user specified a version, then they meant it.
            return Ok(version.clone());
        }

        let user_version = if let Some(pkg) = manifest.and_then(|m| m.package.as_ref()) {
            pkg.version.clone()
        } else {
            None
        };

        let pb = make_spinner!(
            self.quiet,
            format!("Checking if a version of {full_pkg_name} already exists..")
        );

        if let Some(registry_version) = wasmer_backend_api::query::get_package_version(
            client,
            full_pkg_name.to_string(),
            String::from("latest"),
        )
        .await?
        .map(|p| p.version)
        .and_then(|v| semver::Version::from_str(&v).ok())
        {
            spinner_ok!(
                pb,
                format!("Found version {registry_version} of {full_pkg_name}")
            );

            let mut user_version = if let Some(v) = user_version {
                v
            } else {
                registry_version.clone()
            };

            // Must necessarily bump if there's a package with the chosen version with a different hash.
            let must_bump = {
                let maybe_pkg = wasmer_backend_api::query::get_package_version(
                    client,
                    full_pkg_name.to_string(),
                    user_version.to_string(),
                )
                .await?;
                let maybe_hash = maybe_pkg
                    .as_ref()
                    .and_then(|p| p.distribution_v3.pirita_sha256_hash.clone());

                if let Some(hash) = maybe_hash {
                    let registry_package_hash = PackageHash::from_str(&format!("sha256:{hash}"))?;
                    registry_package_hash != self.package_hash
                } else {
                    false
                }
            };

            if user_version <= registry_version {
                if self.bump {
                    user_version = registry_version.clone();
                    user_version.patch = registry_version.patch + 1;
                } else if !self.non_interactive {
                    let theme = ColorfulTheme::default();
                    let mut new_version = registry_version.clone();
                    new_version.patch += 1;
                    if must_bump {
                        eprintln!("{}: Registry already has version {user_version} of {full_pkg_name}, but with different contents.", "Warn".bold().yellow());
                        eprintln!(
                            "{}: Not bumping the version will make this action fail.",
                            "Warn".bold().yellow()
                        );
                        let res = Confirm::with_theme(&theme)
                            .with_prompt(format!("Continue ({user_version} -> {new_version})?"))
                            .interact()?;
                        if res {
                            user_version = new_version.clone();
                            self.update_manifest_version(manifest_path, manifest, &user_version)
                                .await?;
                        } else {
                            anyhow::bail!(
                                "Refusing to map two different releases of {full_pkg_name} to the same version."
                            )
                        }
                    } else {
                        let res = Confirm::with_theme(&theme)
                        .with_prompt(format!("Do you want to bump the package's version ({user_version} -> {new_version})?"))
                            .interact()?;
                        if res {
                            user_version = new_version.clone();
                            self.update_manifest_version(manifest_path, manifest, &user_version)
                                .await?;
                        }
                    }
                }
            }

            Ok(user_version)
        } else {
            pb.finish_and_clear();

            match user_version {
                Some(v) => Ok(v),
                None => {
                    if self.non_interactive {
                        anyhow::bail!(
                            "No package version specified: use --version <package_version>"
                        )
                    } else {
                        let version = crate::utils::prompts::prompt_for_package_version(
                            "Enter the package version",
                            Some("0.1.0"),
                        )?;

                        self.update_manifest_version(manifest_path, manifest, &version)
                            .await?;

                        Ok(version)
                    }
                }
            }
        }
    }

    async fn synthesize_id(
        &mut self,
        client: &WasmerClient,
        manifest: Option<&Manifest>,
        manifest_path: Option<&Path>,
        allow_unnamed: bool,
    ) -> anyhow::Result<Option<NamedPackageId>> {
        let name = match self.get_name(manifest, allow_unnamed)? {
            Some(name) => name,
            None => return Ok(None),
        };

        let namespace = self.get_namespace(client, manifest).await?;
        let full_name = format!("{namespace}/{name}");
        let should_update_name = match &manifest.and_then(|m| m.package.as_ref()) {
            Some(pkg) => match &pkg.name {
                Some(n) => n.as_str() != full_name.as_str(),
                None => true,
            },
            None => true,
        };

        let manifest = if should_update_name {
            self.update_manifest_name(manifest_path, manifest, &full_name)
                .await?
        } else {
            manifest.cloned()
        };

        let version = self
            .get_version(client, manifest.as_ref(), manifest_path, &full_name)
            .await?;

        Ok(Some(NamedPackageId { full_name, version }))
    }

    pub async fn tag(
        &mut self,
        client: &WasmerClient,
        manifest: Option<&Manifest>,
        manifest_path: Option<&Path>,
        after_push: bool,
        allow_unnamed: bool,
    ) -> anyhow::Result<PackageIdent> {
        tracing::debug!("{:?}", self);
        let package_id = self
            .get_package_id(client, &self.package_hash, after_push)
            .await?;
        tracing::info!(
            "The package identifier returned from the registry is {:?}",
            package_id
        );

        let id = match self
            .synthesize_id(client, manifest, manifest_path, allow_unnamed)
            .await?
        {
            Some(id) => id,
            None => return Ok(PackageIdent::Hash(self.package_hash.clone())),
        };

        if self.should_tag(client, &id).await? {
            self.do_tag(client, &id, manifest, &package_id)
                .await
                .map_err(on_error)?;
        }

        Ok(PackageIdent::Named(id.into()))
    }

    // Check if a package with the same hash, namespace, name and version already exists. In such a
    // case, don't tag the package again.
    #[tracing::instrument]
    async fn should_tag(&self, client: &WasmerClient, id: &NamedPackageId) -> anyhow::Result<bool> {
        if self.dry_run {
            if !self.quiet {
                eprintln!("Skipping tagging {id} as `--dry-run` was set");
            }
            return Ok(false);
        }

        let pkg = wasmer_backend_api::query::get_package_version(
            client,
            id.full_name.clone(),
            id.version.to_string(),
        )
        .await?;

        if let Some(hash) = pkg
            .as_ref()
            .and_then(|p| p.distribution_v3.pirita_sha256_hash.as_ref())
        {
            let registry_package_hash = PackageHash::from_str(&format!("sha256:{hash}"))?;
            if registry_package_hash == self.package_hash {
                tracing::info!("decided not to tag as package {pkg:?} already exists");
                return Ok(false);
            }
        }

        Ok(true)
    }
}

#[async_trait::async_trait]
impl AsyncCliCommand for PackageTag {
    type Output = ();

    async fn run_async(mut self) -> Result<Self::Output, anyhow::Error> {
        tracing::info!("Checking if user is logged in");
        let client = login_user(&self.env, !self.non_interactive, "tag a package").await?;

        let (manifest_path, manifest) = match get_manifest(&self.package_path) {
            Ok((manifest_path, manifest)) => {
                tracing::info!("Got manifest at path {}", manifest_path.display());
                (Some(manifest_path), Some(manifest))
            }
            Err(_) => (None, None),
        };

        let id = self
            .tag(
                &client,
                manifest.as_ref(),
                manifest_path.as_deref(),
                false,
                false,
            )
            .await?;

        match id {
            PackageIdent::Named(ref n) => {
                let url = make_package_url(&client, n);
                eprintln!("{} Package URL: {url}", "𖥔".yellow().bold());
            }
            PackageIdent::Hash(ref h) => {
                eprintln!("{} Succesfully tagged package ({h})", "✔".green().bold());
            }
        }

        Ok(())
    }
}
