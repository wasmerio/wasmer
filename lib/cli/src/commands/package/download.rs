use std::path::PathBuf;

use anyhow::{bail, Context};
use dialoguer::console::{style, Emoji};
use futures::TryStreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use tokio::io::AsyncWriteExt;
use wasmer_wasix::runtime::resolver::PackageSpecifier;

use crate::{commands::AsyncCliCommand, opts::ApiOpts};

/// Download a package from the registry.
#[derive(clap::Parser, Debug)]
pub struct PackageDownload {
    #[clap(flatten)]
    #[allow(missing_docs)]
    pub api: ApiOpts,

    /// Verify that the downloaded file is a valid package.
    #[clap(long)]
    validate: bool,

    /// Path where the package file should be written to.
    /// If not specified, the data will be written to stdout.
    #[clap(short = 'o', long)]
    out_path: PathBuf,

    /// Run the download command without any output
    #[clap(long)]
    pub quiet: bool,

    /// The package to download.
    /// Can be:
    /// * a pakage specifier: `namespace/package[@vesion]`
    /// * a URL
    package: PackageSpecifier,
}

static CREATING_OUTPUT_DIRECTORY_EMOJI: Emoji<'_, '_> = Emoji("📁 ", "");
static DOWNLOADING_PACKAGE_EMOJI: Emoji<'_, '_> = Emoji("🌐 ", "");
static RETRIEVING_PACKAGE_INFORMATION_EMOJI: Emoji<'_, '_> = Emoji("📜 ", "");
static VALIDATING_PACKAGE_EMOJI: Emoji<'_, '_> = Emoji("🔍 ", "");
static WRITING_PACKAGE_EMOJI: Emoji<'_, '_> = Emoji("📦 ", "");

#[async_trait::async_trait]
impl AsyncCliCommand for PackageDownload {
    type Output = ();

    async fn run_async(self) -> Result<Self::Output, anyhow::Error> {
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
            style(format!("[{}/{}]", step_num, total_steps))
                .bold()
                .dim(),
            CREATING_OUTPUT_DIRECTORY_EMOJI,
        ));

        step_num += 1;

        if let Some(parent) = self.out_path.parent() {
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
            style(format!("[{}/{}]", step_num, total_steps))
                .bold()
                .dim(),
            RETRIEVING_PACKAGE_INFORMATION_EMOJI
        ));

        step_num += 1;

        let (full_name, version) = match &self.package {
            PackageSpecifier::Registry { full_name, version } => {
                let version = version.to_string();
                let version = if version == "*" { None } else { Some(version) };

                (full_name, version)
            }
            PackageSpecifier::Url(url) => {
                bail!("cannot download a package from a URL: '{}'", url);
            }
            PackageSpecifier::Path(_) => {
                bail!("cannot download a package from a local path");
            }
        };

        let client = self.api.client()?;

        let version = version.unwrap_or_else(|| "latest".to_string());
        let pkg =
            wasmer_api::query::get_package_version(&client, full_name.clone(), version.clone())
                .await
                .with_context(|| {
                    format!(
                    "could not retrieve package information for package '{}' from registry '{}'",
                    full_name,
                    client.graphql_endpoint(),
                )
                })?
                .with_context(|| format!("package '{full_name}@{version}' could not be found"))?;

        let download_url = pkg.distribution.pirita_download_url.context(
            "Package is not available for download. Maybe it is still building or failed to build.",
        )?;

        let tmp_path = self.out_path.with_extension("webc.tmp");
        if let Some(parent) = tmp_path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!("could not create directory '{}'", parent.display())
                })?;
            }
        };
        let mut file = tokio::fs::File::create(&tmp_path)
            .await
            .with_context(|| format!("could not create temporary file '{}'", tmp_path.display()))?;

        let res = client
            .client()
            .get(&download_url)
            .header(http::header::ACCEPT, "application/webc")
            .header(http::header::USER_AGENT, client.user_agent().clone())
            .send()
            .await
            .context("http request failed")?;

        if !res.status().is_success() {
            let status = res.status();
            let body = res
                .text()
                .await
                .unwrap_or_else(|_| "<non-utf8 body>".to_string());
            bail!(
                "could not download package - server returned status code {}\n\n{}",
                status,
                body,
            );
        }

        pb.println(format!(
            "{} {}Downloading package...",
            style(format!("[{}/{}]", step_num, total_steps))
                .bold()
                .dim(),
            DOWNLOADING_PACKAGE_EMOJI
        ));

        step_num += 1;

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
                 one of {:?}, got {ty}",
                accepted_contenttypes
            );
        }

        let mut body = res.bytes_stream();
        while let Some(chunk) = body.try_next().await.context("http request failed")? {
            file.write_all(&chunk)
                .await
                .context("could not write downloaded data to temporary file")?;
            pb.inc(chunk.len() as u64);
        }

        file.sync_all()
            .await
            .context("could not sync temporary file to disk")?;
        std::mem::drop(file);

        if self.validate {
            if !self.quiet {
                println!(
                    "{} {}Validating package...",
                    style(format!("[{}/{}]", step_num, total_steps))
                        .bold()
                        .dim(),
                    VALIDATING_PACKAGE_EMOJI
                );
            }

            step_num += 1;

            webc::compat::Container::from_disk(&tmp_path)
                .context("could not parse downloaded file as a package - invalid download?")?;
        }

        std::fs::rename(&tmp_path, &self.out_path).with_context(|| {
            format!(
                "could not rename temporary file to '{}'",
                self.out_path.display()
            )
        })?;

        pb.println(format!(
            "{} {}Package downloaded to '{}'",
            style(format!("[{}/{}]", step_num, total_steps))
                .bold()
                .dim(),
            WRITING_PACKAGE_EMOJI,
            self.out_path.display()
        ));

        // We're done, so finish the progress bar
        pb.finish();

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::commands::CliCommand;

    use super::*;

    /// Download a package from the dev registry.
    #[test]
    fn test_cmd_package_download() {
        let dir = tempfile::tempdir().unwrap();

        let out_path = dir.path().join("hello.webc");

        let cmd = PackageDownload {
            api: ApiOpts::default(),
            validate: true,
            out_path: out_path.clone(),
            package: "wasmer/hello@0.1.0".parse().unwrap(),
            quiet: true,
        };

        cmd.run().unwrap();

        webc::compat::Container::from_disk(out_path).unwrap();
    }
}
