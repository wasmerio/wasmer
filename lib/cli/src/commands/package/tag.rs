use super::common::PackageSpecifier;
use crate::{
    commands::{
        package::common::{macros::*, *},
        AsyncCliCommand,
    },
    opts::{ApiOpts, WasmerEnv},
    utils::prompt_for_package_version,
};
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Confirm};
use is_terminal::IsTerminal;
use std::path::PathBuf;
use wasmer_api::WasmerClient;
use wasmer_config::package::{Manifest, PackageHash, PackageIdent};

/// Tag an existing package.
#[derive(Debug, clap::Parser)]
pub struct PackageTag {
    #[clap(flatten)]
    pub api: ApiOpts,

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

    ///
    /// Directory containing the `wasmer.toml`, or a custom *.toml manifest file.
    ///
    /// Defaults to current working directory.
    #[clap(name = "path", default_value = ".")]
    pub package_path: PathBuf,
}

impl PackageTag {
    async fn get_namespace(
        &self,
        client: &WasmerClient,
        manifest: &Manifest,
    ) -> anyhow::Result<String> {
        if let Some(owner) = &self.package_namespace {
            return Ok(owner.clone());
        }

        if let Some(pkg) = &manifest.package {
            if let Some(ns) = &pkg.name {
                if let Some(first) = ns.split("/").next() {
                    return Ok(first.to_string());
                }
            }
        }

        if self.non_interactive {
            // if not interactive we can't prompt the user to choose the owner of the app.
            anyhow::bail!("No package namespace specified: use --namespace XXX");
        }

        let user = wasmer_api::query::current_user_with_namespaces(&client, None).await?;
        let owner =
            crate::utils::prompts::prompt_for_namespace("Choose a namespace", None, Some(&user))?;

        Ok(owner.clone())
    }

    async fn synthesize_tagger(
        &self,
        client: &WasmerClient,
        ident: &PackageSpecifier,
        package_release_id: &wasmer_api::types::Id,
    ) -> anyhow::Result<PackageSpecifier> {
        let mut new_ident = ident.clone();

        if let Some(user_version) = &self.package_version {
            match &mut new_ident {
                PackageSpecifier::Hash { .. } => {
                    match (&self.package_name, &self.package_namespace) {
                        (Some(name), Some(namespace)) => {
                            new_ident = PackageSpecifier::Named {
                                namespace: namespace.clone(),
                                name: name.clone(),
                                tag: Tag::Version(user_version.clone()),
                            }
                        }
                        _ => anyhow::bail!(
                            "The flag `--version` was specified, but no namespace and name were given."
                        ),
                    }
                }
                PackageSpecifier::Named { tag, .. } => *tag = Tag::Version(user_version.clone()),
            }
        } else if let Some(user_namespace) = &self.package_namespace {
            match &mut new_ident {
                PackageSpecifier::Hash { hash, namespace } => match &self.package_name {
                    Some(name) => {
                        new_ident = PackageSpecifier::Named {
                            namespace: user_namespace.clone(),
                            name: name.clone(),
                            tag: Tag::Hash(hash.clone()),
                        }
                    }
                    _ => *namespace = user_namespace.clone(),
                },
                PackageSpecifier::Named { namespace, .. } => *namespace = user_namespace.clone(),
            }
        } else if let Some(user_name) = &self.package_name {
            match &mut new_ident {
                PackageSpecifier::Hash { .. } => {
                    anyhow::bail!("The flag `--name` was specified, but no namespace was given.")
                }
                PackageSpecifier::Named { name, .. } => *name = user_name.clone(),
            }
        }

        // At this point, we can resolve the version from the registry if any and if necessary bump
        // the version.
        if let PackageSpecifier::Named {
            name,
            namespace,
            tag,
        } = &mut new_ident
        {
            let full_pkg_name = format!("{namespace}/{name}");
            let pb = make_spinner!(
                self.quiet,
                format!("Checking if a version of {full_pkg_name} already exists..")
            );

            if let Some(p) = wasmer_api::query::get_package_version(
                client,
                full_pkg_name.clone(),
                String::from("latest"),
            )
            .await?
            {
                let registry_version = semver::Version::parse(&p.version)?;
                spinner_ok!(
                    pb,
                    format!("Found version {registry_version} of package {full_pkg_name}")
                );

                tracing::info!(
                    "Package to tag has id = {}, while latest version has id = {}",
                    package_release_id.inner(),
                    p.id.inner()
                );

                if let Tag::Hash(_) = &tag {
                    *tag = Tag::Version(registry_version.clone());
                }

                let user_version: &mut semver::Version = match tag {
                    Tag::Version(v) => v,
                    Tag::Hash(_) => unreachable!(),
                };

                if user_version.clone() <= registry_version && (&p.id != package_release_id) {
                    if self.bump {
                        *user_version = registry_version.clone();
                        user_version.patch = registry_version.patch + 1;
                    } else if !self.non_interactive {
                        let theme = ColorfulTheme::default();
                        let mut new_version = registry_version.clone();
                        new_version.patch += 1;
                        if Confirm::with_theme(&theme).with_prompt(format!("Do you want to bump the package's version ({user_version} -> {new_version})?")).interact()? {
                            *user_version = new_version.clone();
                            // TODO: serialize new version?
                       }
                    }
                }
            } else {
                spinner_ok!(pb, format!("No version of {full_pkg_name} in registry!"));
                let version =
                    prompt_for_package_version("Enter the package version", Some("0.1.0"))?;
                *tag = Tag::Version(version);
            }
        }

        Ok(new_ident)
    }

    async fn do_tag(
        &self,
        client: &WasmerClient,
        ident: &PackageSpecifier,
        manifest: &Manifest,
        package_release_id: &wasmer_api::types::Id,
    ) -> anyhow::Result<PackageSpecifier> {
        tracing::info!(
            "Tagging package with registry id {:?} and specifier {:?}",
            package_release_id,
            ident
        );

        let tagger = self
            .synthesize_tagger(client, ident, package_release_id)
            .await?;

        let pb = make_spinner!(self.quiet, "Tagging package...");

        if let PackageSpecifier::Hash { .. } = &tagger {
            spinner_ok!(pb, "Package is unnamed, no need to tag it");
            return Ok(tagger);
        }

        let PackageSpecifier::Named {
            namespace,
            name,
            tag,
        } = tagger.clone()
        else {
            unreachable!()
        };

        if self.dry_run {
            tracing::info!("No tagging to do here, dry-run is set");
            spinner_ok!(pb, "Skipping tag (dry-run)");
            return Ok(tagger);
        }

        let maybe_description = manifest
            .package
            .as_ref()
            .and_then(|p| p.description.clone());
        let maybe_homepage = manifest.package.as_ref().and_then(|p| p.homepage.clone());
        let maybe_license = manifest.package.as_ref().and_then(|p| p.license.clone());
        let maybe_license_file = manifest
            .package
            .as_ref()
            .and_then(|p| p.license_file.clone())
            .map(|f| f.to_string_lossy().to_string());
        let maybe_readme = manifest
            .package
            .as_ref()
            .and_then(|p| p.readme.clone())
            .map(|f| f.to_string_lossy().to_string());
        let maybe_repository = manifest.package.as_ref().and_then(|p| p.repository.clone());
        let (version, full_name) = (
            match &tag {
                Tag::Version(v) => v.to_string(),
                Tag::Hash(h) => h.to_string(),
            },
            format!("{namespace}/{name}"),
        );

        let private = if let Some(pkg) = &manifest.package {
            Some(pkg.private)
        } else {
            Some(false)
        };

        let manifest_raw = toml::to_string(&manifest)?;

        let r = wasmer_api::query::tag_package_release(
            client,
            maybe_description.as_deref(),
            maybe_homepage.as_deref(),
            maybe_license.as_deref(),
            maybe_license_file.as_deref(),
            &manifest_raw,
            &full_name,
            None,
            package_release_id,
            private,
            maybe_readme.as_deref(),
            maybe_repository.as_deref(),
            &version,
        );

        match r.await? {
            Some(r) => {
                if r.success {
                    spinner_ok!(
                        pb,
                        format!(
                            "Successfully tagged package {}",
                            Into::<PackageIdent>::into(tagger.clone())
                        )
                    );
                    Ok(tagger)
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
    ) -> anyhow::Result<wasmer_api::types::Id> {
        let pb = make_spinner!(self.quiet, "Checking if the package exists..");

        tracing::debug!("Searching for package with hash: {hash}");

        let pkg = match wasmer_api::query::get_package_release(client, &hash.to_string()).await? {
            Some(p) => p,
            None => {
                spinner_err!(pb, "The package is not in the registry!");
                if !self.quiet {
                    eprintln!("\n\nThe package with the required hash does not exist in the selected registry.");
                    let bin_name = bin_name!();
                    let cli = std::env::args()
                        .filter(|s| !s.starts_with("-"))
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
                "Found package {} in the registry",
                hash.to_string()
                    .trim_start_matches("sha256:")
                    .chars()
                    .take(7)
                    .collect::<String>()
            )
        );

        Ok(pkg.id)
    }

    pub async fn tag(
        &self,
        client: &WasmerClient,
        manifest: &Manifest,
    ) -> anyhow::Result<PackageIdent> {
        let namespace = self.get_namespace(client, &manifest).await?;

        let ident = into_specifier(&manifest, &self.package_hash, namespace)?;
        tracing::info!("PackageIdent extracted from manifest is {:?}", ident);

        let package_id = self.get_package_id(&client, &self.package_hash).await?;
        tracing::info!(
            "The package identifier returned from the registry is {:?}",
            package_id
        );

        let ident = self
            .do_tag(&client, &ident, &manifest, &package_id)
            .await
            .map_err(on_error)?;

        Ok(ident.into())
    }
}

#[async_trait::async_trait]
impl AsyncCliCommand for PackageTag {
    type Output = ();

    async fn run_async(self) -> Result<Self::Output, anyhow::Error> {
        tracing::info!("Checking if user is logged in");
        let client =
            login_user(&self.api, &self.env, !self.non_interactive, "tag a package").await?;

        tracing::info!("Loading manifest");
        let (manifest_path, manifest) = get_manifest(&self.package_path)?;
        tracing::info!("Got manifest at path {}", manifest_path.display());

        self.tag(&client, &manifest).await?;

        Ok(())
    }
}
