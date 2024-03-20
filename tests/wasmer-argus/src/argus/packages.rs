use super::Argus;
use crate::ArgusConfig;
use futures::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::{header, Client};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::{fs::File, io::AsyncWriteExt, sync::mpsc::UnboundedSender};
use tracing::*;
use url::Url;
use wasmer_api::{
    query::get_package_versions_stream,
    types::{AllPackageVersionsVars, PackageVersionSortBy, PackageVersionWithPackage},
};

impl Argus {
    /// Fetch all packages from the registry
    #[tracing::instrument(skip(self, s, p))]
    pub async fn fetch_packages(
        &self,
        s: UnboundedSender<PackageVersionWithPackage>,
        p: ProgressBar,
        config: Arc<ArgusConfig>,
    ) -> anyhow::Result<()> {
        info!("starting to fetch packages..");
        let vars = AllPackageVersionsVars {
            sort_by: Some(PackageVersionSortBy::Oldest),
            ..Default::default()
        };

        p.set_style(
            ProgressStyle::with_template("{spinner:.blue} {msg}")
                .unwrap()
                .tick_strings(&["✶", "✸", "✹", "✺", "✹", "✷"]),
        );
        p.enable_steady_tick(Duration::from_millis(1000));

        let mut count = 0;

        let call = get_package_versions_stream(&self.client, vars.clone());
        futures::pin_mut!(call);
        p.set_message("starting to fetch packages..".to_string());

        while let Some(pkgs) = call.next().await {
            let pkgs = match pkgs {
                Ok(pkgs) => pkgs,
                Err(e) => {
                    error!("failed to fetch packages: {e}");
                    p.finish_and_clear();
                    anyhow::bail!("failed to fetch packages: {e}")
                }
            };
            p.set_message(format!("fetched {} packages", count));
            count += pkgs.len();

            for pkg in pkgs {
                if self.to_test(&pkg).await {
                    if let Err(e) = s.send(pkg) {
                        error!("failed to send packages: {e}");
                        p.finish_and_clear();
                        anyhow::bail!("failed to send packages: {e}")
                    };
                }
            }
        }

        p.finish_with_message(format!("fetched {count} packages"));
        info!("finished fetching packages: fetched {count} packages, closing channel");
        drop(s);
        Ok(())
    }

    #[tracing::instrument(skip(p))]
    pub(crate) async fn download_package<'a>(
        test_id: u64,
        path: &'a PathBuf,
        url: &'a Url,
        p: &'a ProgressBar,
    ) -> anyhow::Result<()> {
        info!("downloading package from {} to file {:?}", url, path);
        static APP_USER_AGENT: &str =
            concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);

        if !path.exists() {
            tokio::fs::create_dir_all(path).await?;
        } else if path.exists() && !path.is_dir() {
            anyhow::bail!("path {:?} exists, but it is not a directory!", path)
        }

        let client = Client::builder().user_agent(APP_USER_AGENT).build()?;

        let download_size = {
            let resp = client.head(url.as_str()).send().await?;
            if resp.status().is_success() {
                resp.headers()
                    .get(header::CONTENT_LENGTH)
                    .and_then(|ct_len| ct_len.to_str().ok())
                    .and_then(|ct_len| ct_len.parse().ok())
                    .unwrap_or(0) // Fallback to 0
            } else {
                anyhow::bail!(
                    "Couldn't fetch head from URL {}. Error: {:?}",
                    url,
                    resp.status()
                )
            }
        };

        let request = client.get(url.as_str());

        p.set_length(download_size);

        p.set_style(
            ProgressStyle::default_bar()
                .template(&format!(
                    "[{test_id}] [{{bar:40.cyan/blue}}] {{bytes}}/{{total_bytes}} - {{msg}}"
                ))
                .unwrap()
                .progress_chars("#>-"),
        );

        p.set_message(format!("downloading from {url}"));

        let mut outfile = match File::create(&path.join("package.webc")).await {
            Ok(o) => o,
            Err(e) => {
                error!(
                    "[{test_id}] failed to create file at {:?}. Error: {e}",
                    path.join("package.webc")
                );

                p.finish_and_clear();

                anyhow::bail!(
                    "[{test_id}] failed to create file at {:?}. Error: {e}",
                    path.join("package.webc")
                );
            }
        };
        let mut download = match request.send().await {
            Ok(d) => d,
            Err(e) => {
                error!("[{test_id}] failed to download from URL {url}. Error: {e}");
                p.finish_and_clear();
                anyhow::bail!("[{test_id}] failed to download from URL {url}. Error: {e}");
            }
        };

        loop {
            match download.chunk().await {
                Err(e) => {
                    error!(
                        "[{test_id}] failed to download chunk from {:?}. Error: {e}",
                        download
                    );
                    p.finish_and_clear();
                    anyhow::bail!(
                        "[{test_id}] failed to download chunk from {:?}. Error: {e}",
                        download
                    );
                }
                Ok(chunk) => {
                    if let Some(chunk) = chunk {
                        p.inc(chunk.len() as u64);
                        if let Err(e) = outfile.write(&chunk).await {
                            error!(
                                "[{test_id}] failed to write chunk to file {:?}. Error: {e}",
                                outfile
                            );
                            p.finish_and_clear();
                            anyhow::bail!(
                                "[{test_id}] failed to write chunk to file {:?}. Error: {e}",
                                outfile
                            );
                        };
                    } else {
                        break;
                    }
                }
            }
        }

        outfile.flush().await?;
        drop(outfile);

        Ok(())
    }

    /// Return the complete path to the folder of the test for the package, from the outdir to the
    /// hash
    pub async fn get_path(config: Arc<ArgusConfig>, pkg: &PackageVersionWithPackage) -> PathBuf {
        let hash = match &pkg.distribution.pirita_sha256_hash {
            Some(hash) => hash,
            None => {
                unreachable!("no package without an hash should reach this function!")
            }
        };

        let _namespace = match &pkg.package.namespace {
            Some(ns) => ns.replace('/', "_"),
            None => "unknown_namespace".to_owned(),
        };

        config.outdir.join(hash)
    }

    pub fn get_package_id(pkg: &PackageVersionWithPackage) -> String {
        let namespace = match &pkg.package.namespace {
            Some(namespace) => namespace.replace('/', "_"),
            None => String::from("unknown_namespace"),
        };
        format!(
            "{}/{}_v{}",
            namespace,
            pkg.package.package_name.replace('/', "_"),
            pkg.version
        )
    }
}
