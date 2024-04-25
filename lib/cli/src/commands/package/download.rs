use std::path::PathBuf;

use anyhow::{bail, Context};
use dialoguer::console::{style, Emoji};
use indicatif::{ProgressBar, ProgressStyle};
use tempfile::NamedTempFile;
use wasmer_config::package::{PackageIdent, PackageSource};
use wasmer_registry::wasmer_env::WasmerEnv;

/// Download a package from the registry.
#[derive(clap::Parser, Debug)]
pub struct PackageDownload {
    #[clap(flatten)]
    env: WasmerEnv,

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

        let (download_url, token) = match &self.package {
            PackageSource::Ident(PackageIdent::Named(id)) => {
                let endpoint = self.env.registry_endpoint()?;
                let version = id.version_or_default().to_string();
                let version = if version == "*" { None } else { Some(version) };
                let full_name = id.full_name();
                let token = self.env.get_token_opt().map(|x| x.to_string());

                let package = wasmer_registry::query_package_from_registry(
                    endpoint.as_str(),
                    &full_name,
                    version.as_deref(),
                    token.as_deref(),
                )
                .with_context(|| {
                    format!(
                "could not retrieve package information for package '{}' from registry '{}'",
                full_name, endpoint,
            )
                })?;

                let download_url = package
                    .pirita_url
                    .context("registry does provide a container download container download URL")?;

                (download_url, token)
            }
            PackageSource::Ident(PackageIdent::Hash(hash)) => {
                let endpoint = self.env.registry_endpoint()?;
                let token = self.env.get_token_opt().map(|x| x.to_string());

                let client = wasmer_api::WasmerClient::new(endpoint, "wasmer-cli")?;
                let client = if let Some(token) = &token {
                    client.with_auth_token(token.clone())
                } else {
                    client
                };

                let rt = tokio::runtime::Runtime::new()?;
                let pkg = rt.block_on(wasmer_api::query::get_package_release(&client, &hash.to_string()))?
                    .with_context(|| format!("Package with {hash} does not exist in the registry, or is not accessible"))?;

                (pkg.webc_url, token)
            }
            PackageSource::Path(p) => bail!("cannot download a package from a local path: '{p}'"),
            PackageSource::Url(url) => bail!("cannot download a package from a URL: '{}'", url),
        };

        let client = reqwest::blocking::Client::new();
        let mut b = client
            .get(download_url)
            .header(http::header::ACCEPT, "application/webc");
        if let Some(token) = token {
            b = b.header(http::header::AUTHORIZATION, format!("Bearer {token}"));
        };

        pb.println(format!(
            "{} {}Downloading package...",
            style(format!("[{}/{}]", step_num, total_steps))
                .bold()
                .dim(),
            DOWNLOADING_PACKAGE_EMOJI
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

        let mut tmpfile = NamedTempFile::new_in(self.out_path.parent().unwrap())?;
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

        std::io::copy(&mut pb.wrap_read(res), &mut tmpfile)
            .context("could not write downloaded data to temporary file")?;

        tmpfile.as_file_mut().sync_all()?;

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

            webc::compat::Container::from_disk(tmpfile.path())
                .context("could not parse downloaded file as a package - invalid download?")?;
        }

        tmpfile.persist(&self.out_path).with_context(|| {
            format!(
                "could not persist temporary file to '{}'",
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
    use wasmer_registry::wasmer_env::WASMER_DIR;

    use super::*;

    /// Download a package from the dev registry.
    #[test]
    fn test_cmd_package_download() {
        let dir = tempfile::tempdir().unwrap();

        let out_path = dir.path().join("hello.webc");

        let cmd = PackageDownload {
            env: WasmerEnv::new(WASMER_DIR.clone(), Some("wasmer.wtf".into()), None, None),
            validate: true,
            out_path: out_path.clone(),
            package: "wasmer/hello@0.1.0".parse().unwrap(),
            quiet: true,
        };

        cmd.execute().unwrap();

        webc::compat::Container::from_disk(out_path).unwrap();
    }
}
