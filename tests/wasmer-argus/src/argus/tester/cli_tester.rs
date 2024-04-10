use crate::argus::{Argus, ArgusConfig, Backend};
use indicatif::ProgressBar;
use std::{fs::File, io::BufReader, path::Path, process::Command, sync::Arc};
use tokio::time::{self, Instant};
use tracing::*;
use wasmer_api::types::PackageVersionWithPackage;
use webc::{
    v1::{ParseOptions, WebCOwned},
    v2::read::OwnedReader,
    Container, Version,
};

use super::{TestReport, Tester};

#[allow(unused)]
pub struct CLIRunner<'a> {
    test_id: u64,
    config: Arc<ArgusConfig>,
    p: &'a ProgressBar,
    package: &'a PackageVersionWithPackage,
}

impl<'a> CLIRunner<'a> {
    pub fn new(
        test_id: u64,
        config: Arc<ArgusConfig>,
        p: &'a ProgressBar,
        package: &'a PackageVersionWithPackage,
    ) -> Self {
        Self {
            test_id,
            config,
            p,
            package,
        }
    }

    async fn test_atom(
        &self,
        cli_path: &String,
        atom: &[u8],
        dir_path: &Path,
        atom_id: usize,
    ) -> anyhow::Result<Result<(), String>> {
        if let Err(e) = Command::new(cli_path).arg("-V").output() {
            if let std::io::ErrorKind::NotFound = e.kind() {
                anyhow::bail!("the command '{cli_path}' was not found");
            }
        }

        let atom_path = dir_path.join(format!("atom_{atom_id}.wasm"));
        let output_path = dir_path.join(format!("atom_{atom_id}.wasmu"));

        tokio::fs::write(&atom_path, atom).await?;

        let backend = match self.config.compiler_backend {
            Backend::Llvm => "--llvm",
            Backend::Singlepass => "--singlepass",
            Backend::Cranelift => "--cranelift",
        };

        Ok(
            match std::panic::catch_unwind(move || {
                let mut cmd = Command::new(cli_path);

                let cmd = cmd.args([
                    "compile",
                    atom_path.to_str().unwrap(),
                    backend,
                    "-o",
                    output_path.to_str().unwrap(),
                ]);

                info!("running cmd: {:?}", cmd);

                let out = cmd.output();

                info!("run cmd that gave result: {:#?}", out);

                out
            }) {
                Ok(r) => match r {
                    Ok(_) => Ok(()),
                    Err(e) => Err(e.to_string()),
                },
                Err(_) => Err(String::from("thread panicked")),
            },
        )
    }

    fn ok(&self, version: String, start_time: Instant) -> anyhow::Result<TestReport> {
        Ok(TestReport::new(
            self.package,
            String::from("wasmer_cli"),
            version,
            self.config.compiler_backend.to_string(),
            start_time - Instant::now(),
            Ok(String::from("test passed")),
        ))
    }

    fn err(
        &self,
        version: String,
        start_time: Instant,
        message: String,
    ) -> anyhow::Result<TestReport> {
        Ok(TestReport::new(
            self.package,
            String::from("wasmer_cli"),
            version,
            self.config.compiler_backend.to_string(),
            start_time - Instant::now(),
            Err(message),
        ))
    }

    fn get_id(&self) -> String {
        String::from("wasmer_cli")
    }

    async fn get_version(&self) -> anyhow::Result<String> {
        let cli_path = match &self.config.cli_path {
            Some(ref p) => p.clone(),
            None => String::from("wasmer"),
        };

        let mut cmd = Command::new(&cli_path);
        let cmd = cmd.arg("-V");

        info!("running cmd: {:?}", cmd);

        let out = cmd.output();

        info!("run cmd that gave result: {:?}", out);

        match out {
            Ok(v) => Ok(String::from_utf8(v.stdout)
                .unwrap()
                .replace(' ', "")
                .replace("wasmer", "")
                .trim()
                .to_string()),
            Err(e) => anyhow::bail!("failed to launch cli program {cli_path}: {e}"),
        }
    }
}

#[async_trait::async_trait]
impl<'a> Tester for CLIRunner<'a> {
    async fn run_test(&self) -> anyhow::Result<TestReport> {
        let start_time = time::Instant::now();
        let version = self.get_version().await?;
        let cli_path = match &self.config.cli_path {
            Some(ref p) => p.clone(),
            None => String::from("wasmer"),
        };

        info!("starting test using CLI at {cli_path}");
        let dir_path = Argus::get_path(self.config.clone(), self.package).await;
        let webc_path = dir_path.join("package.webc");

        self.p
            .set_message(format!("unpacking webc at {:?}", webc_path));

        let bytes = std::fs::read(&webc_path)?;

        let webc = match webc::detect(bytes.as_slice()) {
            Ok(Version::V1) => {
                let options = ParseOptions::default();
                let webc = WebCOwned::parse(bytes, &options)?;
                Container::from(webc)
            }
            Ok(Version::V2) => Container::from(OwnedReader::parse(bytes)?),
            Ok(other) => {
                return self.err(version, start_time, format!("Unsupported version, {other}"))
            }
            Err(e) => return self.err(version, start_time, format!("An error occurred: {e}")),
        };

        for (i, atom) in webc.atoms().iter().enumerate() {
            self.p.set_message(format!("testing atom #{i}"));
            if let Err(e) = self
                .test_atom(&cli_path, atom.1.as_slice(), &dir_path, i)
                .await?
            {
                return self.err(version, start_time, e);
            }
        }

        self.ok(version, start_time)
    }

    async fn is_to_test(&self) -> bool {
        let pkg = self.package;
        let version = match self.get_version().await {
            Ok(version) => version,
            Err(e) => {
                error!("skipping test because of error while spawning wasmer CLI command: {e}");
                return false;
            }
        };

        let out_dir = Argus::get_path(self.config.clone(), self.package).await;
        let test_results_path = out_dir.join(format!(
            "result-{}-{}--{}-{}.json",
            self.get_id(),
            version,
            std::env::consts::ARCH,
            std::env::consts::OS,
        ));

        let file = match File::open(test_results_path) {
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
        let report: TestReport = match serde_json::from_reader(reader) {
            Ok(p) => p,
            Err(e) => {
                info!(
                    "re-running test for pkg {:?} as previous-run file failed to be deserialized: {e}",
                    pkg
                );
                return true;
            }
        };

        report.to_test(self.config.clone())
    }
}
