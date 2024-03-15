use super::Tester;
use crate::{argus::result::TestReport, Backend};
use indicatif::ProgressBar;
use std::{path::PathBuf, process::Command};
use tokio::time::{self, Instant};
use tracing::*;
use webc::{
    v1::{ParseOptions, WebCOwned},
    v2::read::OwnedReader,
    Container, Version,
};

#[allow(unused)]
pub struct CLIRunner {
    test_id: u64,
    config: std::sync::Arc<crate::ArgusConfig>,
    webc_path: PathBuf,
    package_name: String,
    start_time: Instant,
}

impl CLIRunner {
    pub fn new(
        test_id: u64,
        config: std::sync::Arc<crate::ArgusConfig>,
        webc_path: PathBuf,
        package_name: &str,
    ) -> Self {
        Self {
            test_id,
            config,
            webc_path,
            package_name: package_name.to_string(),
            start_time: time::Instant::now(),
        }
    }

    pub fn ok(&self) -> anyhow::Result<TestReport> {
        Ok(TestReport::new(
            &self.config,
            Ok(()),
            time::Instant::now() - self.start_time,
        ))
    }

    pub fn err(&self, message: String) -> anyhow::Result<TestReport> {
        Ok(TestReport::new(
            &self.config,
            Err(message),
            time::Instant::now() - self.start_time,
        ))
    }

    async fn test_atom(
        &self,
        cli_path: &String,
        atom: &[u8],
        dir_path: &PathBuf,
        atom_id: usize,
    ) -> anyhow::Result<Result<(), String>> {
        if let Err(e) = Command::new(cli_path).spawn() {
            if let std::io::ErrorKind::NotFound = e.kind() {
                anyhow::bail!("the command '{cli_path}' was not found");
            }
        }

        let atom_path = dir_path.join(format!("atom_{atom_id}.wasm"));

        tokio::fs::write(&atom_path, atom).await?;

        let backend = match self.config.compiler_backend {
            Backend::Llvm => "--llvm",
            Backend::Singlepass => "--singlepass",
            Backend::Cranelift => "--cranelift",
        };

        Ok(
            match std::panic::catch_unwind(|| {
                Command::new(cli_path)
                    .args(["compile", atom_path.to_str().unwrap(), backend])
                    .output()
            }) {
                Ok(r) => match r {
                    Ok(_) => Ok(()),
                    Err(e) => Err(e.to_string()),
                },
                Err(_) => Err(String::from("thread panicked")),
            },
        )
    }
}

#[async_trait::async_trait]
impl Tester for CLIRunner {
    async fn run_test(
        test_id: u64,
        config: std::sync::Arc<crate::ArgusConfig>,
        p: &ProgressBar,
        webc_path: PathBuf,
        package_name: &str,
    ) -> anyhow::Result<crate::argus::result::TestReport> {
        let runner = CLIRunner::new(test_id, config, webc_path, package_name);

        let cli_path = match &runner.config.cli_path {
            Some(ref p) => p.clone(),
            None => String::from("wasmer"),
        };

        info!("starting test using CLI at {cli_path}");
        let mut dir_path = runner.webc_path.clone();
        dir_path.pop();

        p.set_message(format!("unpacking webc at {:?}", runner.webc_path));

        let bytes = std::fs::read(&runner.webc_path)?;

        let webc = match webc::detect(bytes.as_slice()) {
            Ok(Version::V1) => {
                let options = ParseOptions::default();
                let webc = WebCOwned::parse(bytes, &options)?;
                Container::from(webc)
            }
            Ok(Version::V2) => Container::from(OwnedReader::parse(bytes)?),
            Ok(other) => return runner.err(format!("Unsupported version, {other}")),
            Err(e) => return runner.err(format!("An error occurred: {e}")),
        };

        for (i, atom) in webc.atoms().iter().enumerate() {
            if let Err(e) = runner
                .test_atom(&cli_path, atom.1.as_slice(), &dir_path, i)
                .await?
            {
                return runner.err(e);
            }
        }

        runner.ok()
    }
}
