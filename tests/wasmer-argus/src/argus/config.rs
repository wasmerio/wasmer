use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use wasmer::{sys::Features, Engine, NativeEngineExt, Target};

fn get_default_out_path() -> PathBuf {
    let mut path = std::env::current_dir().unwrap();
    path.push("out");
    path
}

fn get_default_token() -> String {
    std::env::var("WASMER_TOKEN").unwrap_or(String::new())
}

#[derive(Debug, Clone, Serialize, Deserialize, ValueEnum, derive_more::Display)]
pub enum Backend {
    LLVM,
    Singlepass,
    Cranelift,
}

impl Backend {
    pub fn to_engine(&self) -> Engine {
        match self {
            Backend::LLVM => todo!(),
            Backend::Singlepass => Engine::new(
                Box::new(wasmer::Singlepass::new()),
                Target::default(),
                Features::default(),
            ),
            Backend::Cranelift => Engine::new(
                Box::new(wasmer::Cranelift::new()),
                Target::default(),
                Features::default(),
            ),
        }
    }
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

    /// The authorization token needed to see packages.
    #[arg(long, default_value_t = get_default_token())]
    pub auth_token: String,

    /// The number of concurrent tests (jobs) to perform
    #[arg(long, default_value = <std::num::NonZeroUsize as Into<usize>>::into(std::thread::available_parallelism().unwrap_or(std::num::NonZeroUsize::new(2).unwrap())).to_string()) ]
    pub jobs: usize,
}

impl ArgusConfig {
    pub fn is_compatible(&self, other: &Self) -> bool {
        self.run_packages == other.run_packages
    }
}
