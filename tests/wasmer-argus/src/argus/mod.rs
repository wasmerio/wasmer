mod config;
mod packages;
mod tester;

use self::tester::{TestReport, Tester};
pub use config::*;
use indicatif::{MultiProgress, ProgressBar};
use reqwest::header::CONTENT_TYPE;
use std::{fs::OpenOptions, io::Write as _, ops::AddAssign, path::Path, sync::Arc, time::Duration};
use tokio::{
    sync::{
        mpsc::{self, UnboundedSender},
        Mutex, Semaphore,
    },
    task::JoinSet,
};
use tracing::*;
use url::Url;
use wasmer_backend_api::{types::PackageVersionWithPackage, WasmerClient};

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
        let (successes_sx, successes_rx) = mpsc::unbounded_channel();
        let (failures_sx, failures_rx) = mpsc::unbounded_channel();

        let mut pool = JoinSet::new();
        let c = Arc::new(self.config.clone());

        {
            let this = self.clone();
            let bar = m.add(ProgressBar::new(0));
            pool.spawn(async move {
                this.fetch_packages(s, bar, c.clone(), successes_rx, failures_rx)
                    .await
            });
        }

        let mut count = 0;
        let successes = Arc::new(Mutex::new(0));
        let failures = Arc::new(Mutex::new(0));

        let c = Arc::new(self.config.clone());
        let sem = Arc::new(Semaphore::new(self.config.jobs));

        while let Some(pkg) = r.recv().await {
            let c = c.clone();
            let bar = m.add(ProgressBar::new(0));
            let permit = Arc::clone(&sem).acquire_owned().await;
            let successes_sx = successes_sx.clone();
            let failures_sx = failures_sx.clone();
            let failures = failures.clone();
            let successes = successes.clone();

            pool.spawn(async move {
                let _permit = permit;
                match Argus::test(count, c, &pkg, bar, successes_sx, failures_sx).await {
                    Err(e) => {
                        failures.lock().await.add_assign(1);
                        Err(e)
                    }
                    Ok(true) => {
                        successes.lock().await.add_assign(1);
                        Ok(())
                    }
                    Ok(false) => {
                        failures.lock().await.add_assign(1);
                        Ok(())
                    }
                }
            });

            count += 1;
        }

        while let Some(t) = pool.join_next().await {
            if let Err(e) = t {
                error!("{:?}", e)
            }
        }

        if let Some(webhook_url) = self.config.webhook_url {
            let url = url::Url::parse(&webhook_url)?;
            reqwest::Client::new()
                .post(url)
                .header(CONTENT_TYPE, "application/json")
                .body(format!(
                    r#"{{"text":"Argus run report: {} tests succeeded, {} failed"}}"#,
                    successes.lock().await,
                    failures.lock().await
                ))
                .send()
                .await?;
        }

        info!("done!");
        Ok(())
    }

    /// Perform the test for a single package
    async fn test(
        test_id: u64,
        config: Arc<ArgusConfig>,
        package: &PackageVersionWithPackage,
        p: ProgressBar,
        successes_sx: UnboundedSender<()>,
        failures_sx: UnboundedSender<()>,
    ) -> anyhow::Result<bool> {
        p.set_style(
            indicatif::ProgressStyle::with_template(&format!(
                "[{test_id}] {{spinner:.blue}} {{msg}}"
            ))
            .unwrap()
            .tick_strings(&["✶", "✸", "✹", "✺", "✹", "✷", "✶"]),
        );

        p.enable_steady_tick(Duration::from_millis(100));

        let package_name = Argus::get_package_id(package);
        let webc_v2_url: Url = match &package.distribution_v2.pirita_download_url {
            Some(url) => url.parse().unwrap(),
            None => {
                info!("package {} has no download url, skipping", package_name);
                p.finish_and_clear();
                return Ok(true);
            }
        };

        let webc_v3_url: Url = match &package.distribution_v3.pirita_download_url {
            Some(url) => url.parse().unwrap(),
            None => {
                info!("package {} has no download url, skipping", package_name);
                p.finish_and_clear();
                return Ok(true);
            }
        };

        p.set_message(format!("[{test_id}] testing package {package_name}"));

        let path = Argus::get_path(config.clone(), package).await;
        p.set_message(format!(
            "testing package {package_name} -- path to download to is: {path:?}",
        ));

        #[cfg(not(feature = "wasmer_lib"))]
        let runner = Box::new(tester::cli_tester::CLIRunner::new(
            test_id, config, &p, package,
        )) as Box<dyn Tester>;

        #[cfg(feature = "wasmer_lib")]
        let runner = if config.use_lib {
            Box::new(tester::lib_tester::LibRunner::new(
                test_id, config, &p, package,
            )) as Box<dyn Tester>
        } else {
            Box::new(tester::cli_tester::CLIRunner::new(
                test_id, config, &p, package,
            )) as Box<dyn Tester>
        };

        if !runner.is_to_test().await {
            return Ok(true);
        }

        Argus::download_webcs(test_id, &path, &webc_v2_url, &webc_v3_url, &p).await?;

        info!("package downloaded!");

        p.reset();
        p.set_style(
            indicatif::ProgressStyle::with_template(&format!(
                "[{test_id}/{package_name}] {{spinner:.blue}} {{msg}}"
            ))
            .unwrap()
            .tick_strings(&["✶", "✸", "✹", "✺", "✹", "✷", "✶"]),
        );

        p.enable_steady_tick(Duration::from_millis(100));

        p.set_message("package downloaded");

        let report = runner.run_test().await?;

        let outcome = report.outcome.is_ok();

        if outcome {
            successes_sx.send(())?;
        } else {
            failures_sx.send(())?;
        };
        Argus::write_report(&path, report).await?;

        p.finish_with_message(format!("test for package {package_name} done!"));
        p.finish_and_clear();

        Ok(outcome)
    }

    /// Checks whether or not the package should be tested
    async fn to_test(&self, pkg: &PackageVersionWithPackage) -> bool {
        let name = Argus::get_package_id(pkg);

        info!("checking if package {name} needs to be tested or not");

        let dir_path = std::path::PathBuf::from(&self.config.outdir);

        if !dir_path.exists() {
            return true;
        }

        if pkg.distribution_v2.pirita_sha256_hash.is_none() {
            info!("skipping test for {name} as it has no hash");
            return false;
        }

        true
    }

    #[tracing::instrument]
    async fn write_report(path: &Path, result: TestReport) -> anyhow::Result<()> {
        let test_results_path = path.join(format!(
            "result-{}-{}--{}-{}.json",
            result.runner_id,
            result.runner_version,
            std::env::consts::ARCH,
            std::env::consts::OS,
        ));

        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(test_results_path)?;

        file.write_all(serde_json::to_string(&result).unwrap().as_bytes())?;
        Ok(())
    }
}
