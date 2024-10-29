use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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

    /// Whether or not this run should use the linked [`wasmer-backend-api`] library instead of the CLI.
    #[cfg(feature = "wasmer_lib")]
    #[arg(long, conflicts_with = "cli_path")]
    pub use_lib: bool,

    /// The webhook to use when sending the test outcome.
    #[arg(long)]
    pub webhook_url: Option<String>,
}
