mod config;
mod packages;
mod result;

pub use config::*;

use indicatif::{MultiProgress, ProgressBar};
use std::{
    fs::{File, OpenOptions},
    io::{BufReader, Write as _},
    path::Path,
    sync::Arc,
    time::Duration,
};
use tokio::{
    sync::{mpsc, Semaphore},
    task::JoinSet,
    time,
};
use tracing::*;
use url::Url;
use wasmer_api::{types::PackageVersionWithPackage, WasmerClient};
use webc::{
    v1::{ParseOptions, WebCOwned},
    v2::read::OwnedReader,
    Container, Version,
};

use crate::argus::result::TestResults;

use self::result::TestReport;

#[derive(Debug, Clone)]
pub struct Argus {
    pub config: ArgusConfig,
    pub client: WasmerClient,
}

impl TryFrom<ArgusConfig> for Argus {
    type Error = anyhow::Error;

    fn try_from(config: ArgusConfig) -> Result<Self, Self::Error> {
        let client = WasmerClient::new(Url::parse(&config.registry_url)?, "wasmer-argus")?;

        let client = client.with_auth_token(config.auth_token.clone());
        Ok(Argus { client, config })
    }
}

impl Argus {
    /// Start the testsuite using the configuration in [`Self::config`]
    pub async fn run(self) -> anyhow::Result<()> {
        info!("fetching packages from {}", self.config.registry_url);

        let m = MultiProgress::new();
        let (s, mut r) = mpsc::unbounded_channel();

        let mut pool = JoinSet::new();

        {
            let this = self.clone();
            let bar = m.add(ProgressBar::new(0));

            pool.spawn(async move { this.fetch_packages(s, bar).await });
        }

        let c = Arc::new(self.config.clone());

        let mut count = 0;

        let sem = Arc::new(Semaphore::new(self.config.jobs));

        while let Some(pkg) = r.recv().await {
            let c = c.clone();
            let bar = m.add(ProgressBar::new(0));
            let permit = Arc::clone(&sem).acquire_owned().await;

            pool.spawn(async move {
                let _permit = permit;
                Argus::test(count, c, &pkg, bar).await
            });

            count += 1;
        }

        while let Some(t) = pool.join_next().await {
            if let Err(e) = t {
                error!("task failed: {e}")
            }
        }

        info!("done!");
        Ok(())
    }

    /// The actual test
    async fn test(
        test_id: u64,
        config: Arc<ArgusConfig>,
        pkg: &PackageVersionWithPackage,
        p: ProgressBar,
    ) -> anyhow::Result<()> {
        p.set_style(
            indicatif::ProgressStyle::with_template(&format!(
                "[{test_id}] {{spinner:.blue}} {{msg}}"
            ))
            .unwrap()
            .tick_strings(&["✶", "✸", "✹", "✺", "✹", "✷"]),
        );

        p.enable_steady_tick(Duration::from_millis(100));

        let name = Argus::get_package_id(pkg);

        let webc_url: Url = match &pkg.distribution.pirita_download_url {
            Some(url) => url.parse().unwrap(),
            None => {
                info!("package {} has no download url, skipping", name);
                p.finish_and_clear();
                return Ok(());
            }
        };

        p.set_message(format!("[{test_id}] testing package {name}",));

        let path = Argus::get_path(config.clone(), pkg).await;
        p.set_message(format!(
            "testing package {name} -- path to download to is: {:?}",
            path
        ));

        Argus::download_package(test_id, &path, &webc_url, &p).await?;

        info!("package downloaded!");

        p.reset();
        p.set_style(
            indicatif::ProgressStyle::with_template(&format!(
                "[{test_id}] {{spinner:.blue}} {{msg}}"
            ))
            .unwrap()
            .tick_strings(&["✶", "✸", "✹", "✺", "✹", "✷"]),
        );

        p.enable_steady_tick(Duration::from_millis(100));

        p.set_message("package downloaded");
        let filepath = path.join("package.webc");

        let start = time::Instant::now();

        let test_exec_result = std::panic::catch_unwind(|| {
            p.set_message("reading webc bytes from filesystem");
            let bytes = std::fs::read(&filepath)?;
            let store = wasmer::Store::new(config.compiler_backend.to_engine());

            let webc = match webc::detect(bytes.as_slice()) {
                Ok(Version::V1) => {
                    let options = ParseOptions::default();
                    let webc = WebCOwned::parse(bytes, &options)?;
                    Container::from(webc)
                }
                Ok(Version::V2) => Container::from(OwnedReader::parse(bytes)?),
                Ok(other) => anyhow::bail!("Unsupported version, {other}"),
                Err(e) => anyhow::bail!("An error occurred: {e}"),
            };

            p.set_message("created webc");

            for atom in webc.atoms().iter() {
                info!(
                    "creating module for atom {} with length {}",
                    atom.0,
                    atom.1.len()
                );
                p.set_message(format!(
                    "-- {name} -- creating module for atom {} (has length {} bytes)",
                    atom.0,
                    atom.1.len()
                ));
                wasmer::Module::new(&store, atom.1.as_slice())?;
            }

            Ok(())
        });

        let res = match test_exec_result {
            Ok(r) => match r {
                Ok(_) => Ok(()),
                Err(e) => Err(format!("{e}")),
            },
            Err(e) => Err(format!("{:?}", e)),
        };

        let time = start - time::Instant::now();

        let report = TestReport::new(config.as_ref(), res, time);

        Argus::write_report(&path, report).await?;

        p.finish_with_message(format!("test for package {name} done!"));
        p.finish_and_clear();

        Ok(())
    }

    /// Checks whether or not the package should be tested
    ///
    /// This is done by checking if it was already tested in a compatible (i.e. same backend)
    /// previous run by searching for the a directory with the package name in the directory
    /// [`PackageVersionWithPackage::package`] with the same `pirita_sha256_hash` as in
    /// [`PackageVersionWithPackage::distribution`] that contains a file that matches the current
    /// configuration.
    ///
    /// For example, given a package such as
    /// ```
    /// {
    ///     "package": {
    ///         "package_name": "any/mytest",
    ///         ...
    ///     },
    ///     "distribution": {
    ///         "pirita_sha256_hash":
    ///             "47945b31a4169e6c82162d29e3f54cbf7cb979c8e84718a86dec1cc0f6c19890"
    ///     }
    ///     ...
    /// }
    /// ```
    ///
    /// this function will check if there is a file with path
    /// `any_mytest/47945b31a4169e6c82162d29e3f54cbf7cb979c8e84718a86dec1cc0f6c19890.json`
    /// in `outdir` as prescribed by [`Self::config`]. If the file contains a compatible test run,
    /// it returns `false`.
    /// If the output directory does not exists, this function returns `true`.
    async fn to_test(&self, pkg: &PackageVersionWithPackage) -> bool {
        let name = Argus::get_package_id(pkg);

        info!("checking if package {name} needs to be tested or not");

        let dir_path = std::path::PathBuf::from(&self.config.outdir);

        if !dir_path.exists() {
            return true;
        }

        if pkg.distribution.pirita_sha256_hash.is_none() {
            info!("skipping test for {name} as it has no hash");
            return false;
        }

        let path = Argus::get_path(Arc::new(self.config.clone()), pkg)
            .await
            .join("results.json");
        if !path.exists() {
            return true;
        }

        let file = match File::open(path) {
            Ok(file) => file,
            Err(e) => {
                info!(
                    "re-running test for pkg {:?} as previous-run file failed to open: {e}",
                    pkg
                );
                return true;
            }
        };

        let reader = BufReader::new(file);
        let prev_run: TestResults = match serde_json::from_reader(reader) {
            Ok(p) => p,
            Err(e) => {
                info!(
                    "re-running test for pkg {:?} as previous-run file failed to be deserialized: {e}",
                    pkg
                );
                return true;
            }
        };

        !prev_run.has(&self.config)
    }

    async fn write_report(path: &Path, report: TestReport) -> anyhow::Result<()> {
        let test_results_path = path.join("results.json");

        let mut test_results = if test_results_path.exists() {
            let s = tokio::fs::read_to_string(&test_results_path).await?;
            serde_json::from_str(&s)?
        } else {
            TestResults::default()
        };

        test_results.add(report);

        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&test_results_path)?;
        file.write_all(serde_json::to_string(&test_results).unwrap().as_bytes())?;
        Ok(())
    }
}
