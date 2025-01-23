use std::{env::current_dir, path::PathBuf};

use anyhow::{bail, Context};
use dialoguer::console::{style, Emoji};
use indicatif::{ProgressBar, ProgressStyle};
use tempfile::NamedTempFile;
use wasmer_config::package::{PackageIdent, PackageSource};
use wasmer_package::utils::from_disk;

use crate::config::WasmerEnv;

/// Download a package from the registry.
#[derive(clap::Parser, Debug)]
pub struct PackageDownload {
    #[clap(flatten)]
    pub env: WasmerEnv,

    /// Verify that the downloaded file is a valid package.
    #[clap(long)]
    validate: bool,

    /// Path where the package file should be written to.
    /// If not specified, the data will be written to stdout.
    #[clap(short = 'o', long)]
    out_path: Option<PathBuf>,

    /// Run the download command without any output
    #[clap(long)]
    pub quiet: bool,

    /// The package to download.
    package: PackageSource,
}

static CREATING_OUTPUT_DIRECTORY_EMOJI: Emoji<'_, '_> = Emoji("📁 ", "");
static DOWNLOADING_PACKAGE_EMOJI: Emoji<'_, '_> = Emoji("🌐 ", "");
static RETRIEVING_PACKAGE_INFORMATION_EMOJI: Emoji<'_, '_> = Emoji("📜 ", "");
static VALIDATING_PACKAGE_EMOJI: Emoji<'_, '_> = Emoji("🔍 ", "");
static WRITING_PACKAGE_EMOJI: Emoji<'_, '_> = Emoji("📦 ", "");

impl PackageDownload {
    pub(crate) fn execute(&self) -> Result<(), anyhow::Error> {
        let total_steps = if self.validate { 5 } else { 4 };
        let mut step_num = 1;

        // Setup the progress bar
        let pb = if self.quiet {
            ProgressBar::hidden()
        } else {
            ProgressBar::new_spinner()
        };

        pb.set_style(ProgressStyle::with_template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})")
                                .unwrap()
                                .progress_chars("#>-"));

        pb.println(format!(
            "{} {}Creating output directory...",
            style(format!("[{step_num}/{total_steps}]")).bold().dim(),
            CREATING_OUTPUT_DIRECTORY_EMOJI,
        ));

        step_num += 1;

        if let Some(parent) = self.out_path.as_ref().and_then(|p| p.parent()) {
            match parent.metadata() {
                Ok(m) => {
                    if !m.is_dir() {
                        bail!(
                            "parent of output file is not a directory: '{}'",
                            parent.display()
                        );
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    std::fs::create_dir_all(parent)
                        .context("could not create parent directory of output file")?;
                }
                Err(err) => return Err(err.into()),
            }
        };

        pb.println(format!(
            "{} {}Retrieving package information...",
            style(format!("[{step_num}/{total_steps}]")).bold().dim(),
            RETRIEVING_PACKAGE_INFORMATION_EMOJI
        ));

        step_num += 1;

        let (download_url, ident, filename) = match &self.package {
            PackageSource::Ident(PackageIdent::Named(id)) => {
                // caveat: client_unauthennticated will use a token if provided, it
                // just won't fail if none is present. So, _unauthenticated() can actually
                // produce an authenticated client.
                let client = self.env.client_unauthennticated()?;

                let version = id.version_or_default().to_string();
                let version = if version == "*" {
                    String::from("latest")
                } else {
                    version.to_string()
                };
                let full_name = id.full_name();

                let rt = tokio::runtime::Runtime::new()?;
                let package = rt
                    .block_on(wasmer_backend_api::query::get_package_version(
                        &client,
                        full_name.clone(),
                        version.clone(),
                    ))?
                    .with_context(|| {
                        format!(
                    "could not retrieve package information for package '{}' from registry '{}'",
                    full_name, client.graphql_endpoint(),
                )
                    })?;

                let download_url = package
                    .distribution_v3
                    .pirita_download_url
                    .context("registry did not provide a container download URL")?;

                let ident = format!("{}@{}", full_name, package.version);
                let filename = if let Some(ns) = &package.package.namespace {
                    format!(
                        "{}--{}@{}.webc",
                        ns.clone(),
                        package.package.package_name,
                        package.version
                    )
                } else {
                    format!("{}@{}.webc", package.package.package_name, package.version)
                };

                (download_url, ident, filename)
            }
            PackageSource::Ident(PackageIdent::Hash(hash)) => {
                // caveat: client_unauthennticated will use a token if provided, it
                // just won't fail if none is present. So, _unauthenticated() can actually
                // produce an authenticated client.
                let client = self.env.client_unauthennticated()?;

                let rt = tokio::runtime::Runtime::new()?;
                let pkg = rt.block_on(wasmer_backend_api::query::get_package_release(&client, &hash.to_string()))?
                    .with_context(|| format!("Package with {hash} does not exist in the registry, or is not accessible"))?;

                let ident = hash.to_string();
                let filename = format!("{hash}.webc");

                (pkg.webc_url, ident, filename)
            }
            PackageSource::Path(p) => bail!("cannot download a package from a local path: '{p}'"),
            PackageSource::Url(url) => bail!("cannot download a package from a URL: '{}'", url),
        };

        let builder = {
            let mut builder = reqwest::blocking::ClientBuilder::new();
            if let Some(proxy) = self.env.proxy()? {
                builder = builder.proxy(proxy);
            }
            builder
        };
        let client = builder.build().context("failed to create reqwest client")?;

        let b = client
            .get(download_url)
            .header(http::header::ACCEPT, "application/webc");

        pb.println(format!(
            "{} {DOWNLOADING_PACKAGE_EMOJI}Downloading package {ident} ...",
            style(format!("[{step_num}/{total_steps}]")).bold().dim(),
        ));

        step_num += 1;

        let res = b
            .send()
            .context("http request failed")?
            .error_for_status()
            .context("http request failed with non-success status code")?;

        let webc_total_size = res
            .headers()
            .get(http::header::CONTENT_LENGTH)
            .and_then(|t| t.to_str().ok())
            .and_then(|t| t.parse::<u64>().ok())
            .unwrap_or_default();

        if webc_total_size == 0 {
            bail!("Package is empty");
        }

        // Set the length of the progress bar
        pb.set_length(webc_total_size);

        let mut tmpfile = if let Some(parent) = self.out_path.as_ref().and_then(|p| p.parent()) {
            NamedTempFile::new_in(parent)?
        } else {
            NamedTempFile::new()?
        };
        let accepted_contenttypes = vec![
            "application/webc",
            "application/octet-stream",
            "application/wasm",
        ];
        let ty = res
            .headers()
            .get(http::header::CONTENT_TYPE)
            .and_then(|t| t.to_str().ok())
            .unwrap_or_default();
        if !(accepted_contenttypes.contains(&ty)) {
            eprintln!(
                "Warning: response has invalid content type - expected \
                 one of {accepted_contenttypes:?}, got {ty}",
            );
        }

        std::io::copy(&mut pb.wrap_read(res), &mut tmpfile)
            .context("could not write downloaded data to temporary file")?;

        tmpfile.as_file_mut().sync_all()?;

        if self.validate {
            if !self.quiet {
                println!(
                    "{} {VALIDATING_PACKAGE_EMOJI}Validating package...",
                    style(format!("[{step_num}/{total_steps}]")).bold().dim(),
                );
            }

            step_num += 1;

            from_disk(tmpfile.path())
                .context("could not parse downloaded file as a package - invalid download?")?;
        }

        let out_path = if let Some(out_path) = &self.out_path {
            out_path.clone()
        } else {
            current_dir()?.join(filename)
        };

        tmpfile.persist(&out_path).with_context(|| {
            format!(
                "could not persist temporary file to '{}'",
                out_path.display()
            )
        })?;

        pb.println(format!(
            "{} {WRITING_PACKAGE_EMOJI}Package downloaded to '{}'",
            style(format!("[{step_num}/{total_steps}]")).bold().dim(),
            out_path.display()
        ));

        // We're done, so finish the progress bar
        pb.finish();

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Download a package from the dev registry.
    #[test]
    fn test_cmd_package_download() {
        let dir = tempfile::tempdir().unwrap();

        let out_path = dir.path().join("hello.webc");

        let cmd = PackageDownload {
            env: WasmerEnv::new(
                crate::config::DEFAULT_WASMER_CACHE_DIR.clone(),
                crate::config::DEFAULT_WASMER_CACHE_DIR.clone(),
                None,
                Some("https://registry.wasmer.io/graphql".to_owned().into()),
            ),
            validate: true,
            out_path: Some(out_path.clone()),
            package: "wasmer/hello@0.1.0".parse().unwrap(),
            quiet: true,
        };

        cmd.execute().unwrap();

        from_disk(out_path).unwrap();
    }
}
