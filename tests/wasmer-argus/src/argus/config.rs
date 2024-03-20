use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, process::Command};
use tracing::*;

fn get_default_out_path() -> PathBuf {
    let mut path = std::env::current_dir().unwrap();
    path.push("out");
    path
}

fn get_default_token() -> String {
    std::env::var("WASMER_TOKEN").unwrap_or_default()
}

fn get_default_jobs() -> usize {
    std::thread::available_parallelism()
        .unwrap_or(std::num::NonZeroUsize::new(2).unwrap())
        .into()
}

#[derive(Debug, Clone, Serialize, Deserialize, ValueEnum, derive_more::Display)]
pub enum Backend {
    Llvm,
    Singlepass,
    Cranelift,
}

/// Fetch and test packages from a WebContainer registry.
#[derive(Debug, clap::Parser, Clone, Serialize, Deserialize)]
#[command(version, about, long_about = None)]
pub struct ArgusConfig {
    /// The GraphQL endpoint of the registry to test
    #[arg(
        short,
        long,
        default_value_t = String::from("http://registry.wasmer.io/graphql")
    )]
    pub registry_url: String,

    /// The backend to test the compilation against
    #[arg(short = 'b', long = "backend", value_enum, default_value_t = Backend::Singlepass)]
    pub compiler_backend: Backend,

    /// Whether or not to run packages during tests
    #[arg(long = "run")]
    pub run_packages: bool,

    /// The output directory
    #[arg(short = 'o', long, default_value = get_default_out_path().into_os_string())]
    pub outdir: std::path::PathBuf,

    /// The authorization token needed to see packages
    #[arg(long, default_value_t = get_default_token())]
    pub auth_token: String,

    /// The number of concurrent tests (jobs) to perform
    #[arg(long, default_value_t = get_default_jobs()) ]
    pub jobs: usize,

    /// The path to the CLI command to use. [default will be searched in $PATH: "wasmer"]
    #[arg(long)]
    pub cli_path: Option<String>,

    /// Whether or not this run should use the linked [`wasmer-api`] library instead of the CLI.
    #[cfg(feature = "wasmer_lib")]
    #[arg(long, conflicts_with = "cli_path")]
    pub use_lib: bool,
}

impl ArgusConfig {
    pub fn is_compatible(&self, other: &Self) -> bool {
        // Ideally, in the future, we could add more features that could make us conclude that we
        // need to run tests again.
        self.run_packages == other.run_packages
    }

    pub fn wasmer_version(&self) -> String {
        #[cfg(feature = "wasmer_lib")]
        if self.use_lib {
            return format!("wasmer_lib/{}", env!("CARGO_PKG_VERSION"));
        }

        let cli_path = match &self.cli_path {
            Some(p) => p,
            None => "wasmer",
        };

        let mut cmd = Command::new(cli_path);
        let cmd = cmd.arg("-V");

        info!("running cmd: {:?}", cmd);

        let out = cmd.output();

        info!("run cmd that gave result: {:?}", out);

        match out {
            Ok(v) => String::from_utf8(v.stdout)
                .unwrap()
                .replace(' ', "/")
                .trim()
                .to_string(),
            Err(e) => panic!("failed to launch cli program {cli_path}: {e}"),
        }
    }
}
