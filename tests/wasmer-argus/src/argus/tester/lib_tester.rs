use super::Tester;
use crate::{argus::result::TestReport, Backend};
use indicatif::ProgressBar;
use std::path::PathBuf;
use tokio::time;
use tracing::*;
use wasmer::{sys::Features, Engine, NativeEngineExt, Target};
use webc::{
    v1::{ParseOptions, WebCOwned},
    v2::read::OwnedReader,
    Container, Version,
};

pub struct LibRunner;

#[async_trait::async_trait]
impl Tester for LibRunner {
    async fn run_test(
        test_id: u64,
        config: std::sync::Arc<crate::ArgusConfig>,
        p: &ProgressBar,
        webc_path: PathBuf,
        package_name: &str,
    ) -> anyhow::Result<crate::argus::result::TestReport> {
        let start = time::Instant::now();

        let test_exec_result = std::panic::catch_unwind(|| {
            p.set_message("reading webc bytes from filesystem");
            let bytes = std::fs::read(&webc_path)?;
            let store = wasmer::Store::new(Self::backend_to_engine(&config.compiler_backend));

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
                    "-- {package_name} -- creating module for atom {} (has length {} bytes)",
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

        Ok(TestReport::new(config.as_ref(), res, time))
    }
}

impl LibRunner {
    pub fn backend_to_engine(backend: &Backend) -> Engine {
        match backend {
            Backend::Llvm => Engine::new(
                Box::new(wasmer::LLVM::new()),
                Target::default(),
                Features::default(),
            ),
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
