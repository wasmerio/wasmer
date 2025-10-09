use super::{TestReport, Tester};
use crate::argus::{Argus, ArgusConfig, Backend};
use indicatif::ProgressBar;
use std::{fs::File, io::BufReader, sync::Arc};
use tokio::time;
use tracing::*;
use wasmer::{Engine, NativeEngineExt, Target, sys::Features};
use wasmer_backend_api::types::PackageVersionWithPackage;
use webc::{Container, Version, v2::read::OwnedReader, v3::read::OwnedReader as OwnedReaderV3};

pub struct LibRunner<'a> {
    pub test_id: u64,
    pub config: Arc<ArgusConfig>,
    pub p: &'a ProgressBar,
    pub package: &'a PackageVersionWithPackage,
}

impl<'a> LibRunner<'a> {
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

    fn get_id(&self) -> String {
        String::from("wasmer_lib")
    }

    fn get_version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }
}

#[async_trait::async_trait]
impl<'a> Tester for LibRunner<'a> {
    async fn run_test(&self) -> anyhow::Result<TestReport> {
        let package_id = crate::Argus::get_package_id(self.package);

        let start = time::Instant::now();
        let dir_path = Argus::get_path(self.config.clone(), self.package).await;
        let webc_v2_path = dir_path.join("package_v2.webc");
        let webc_v3_path = dir_path.join("package_v3.webc");

        let test_exec_result = std::panic::catch_unwind(|| {
            self.p.set_message("reading webc bytes from filesystem");
            let bytes = std::fs::read(&webc_v2_path)?;
            let store = wasmer::Store::new(Self::backend_to_engine(&self.config.compiler_backend));

            let webc = match webc::detect(bytes.as_slice()) {
                Ok(Version::V2) => Container::from(OwnedReader::parse(bytes)?),
                Ok(other) => anyhow::bail!("Unsupported version, {other}"),
                Err(e) => anyhow::bail!("An error occurred: {e}"),
            };

            self.p.set_message("created webc");

            for atom in webc.atoms().iter() {
                info!(
                    "creating module for atom {} with length {}",
                    atom.0,
                    atom.1.len()
                );
                self.p.set_message(format!(
                    "[{package_id}] creating module for atom {} (has length {} bytes)",
                    atom.0,
                    atom.1.len()
                ));
                wasmer::Module::new(&store, atom.1.as_slice())?;
            }

            self.p.set_message("reading webc bytes from filesystem");
            let bytes = std::fs::read(&webc_v3_path)?;
            let store = wasmer::Store::new(Self::backend_to_engine(&self.config.compiler_backend));

            let webc = match webc::detect(bytes.as_slice()) {
                Ok(Version::V3) => {
                    let c = Container::from(OwnedReaderV3::parse(bytes)?);
                    println!("\n\n\n c: {:?} \n\n\n", c.version());
                    c
                }
                Ok(other) => anyhow::bail!("Unsupported version, {other}"),
                Err(e) => anyhow::bail!("An error occurred: {e}"),
            };

            self.p.set_message("created webc");

            for atom in webc.atoms().iter() {
                info!(
                    "creating module for atom {} with length {}",
                    atom.0,
                    atom.1.len()
                );
                self.p.set_message(format!(
                    "[{package_id}] creating module for atom {} (has length {} bytes)",
                    atom.0,
                    atom.1.len()
                ));
                wasmer::Module::new(&store, atom.1.as_slice())?;
            }

            Ok(())
        });

        let outcome = match test_exec_result {
            Ok(r) => match r {
                Ok(_) => {
                    let v2_file = std::fs::File::open(&webc_v2_path)?;
                    let v3_file = std::fs::File::open(&webc_v3_path)?;

                    if let Err(e) = webc::migration::are_semantically_equivalent(
                        shared_buffer::OwnedBuffer::from_file(&v2_file)?,
                        shared_buffer::OwnedBuffer::from_file(&v3_file)?,
                    ) {
                        Err(e.to_string())
                    } else {
                        Ok(String::from("test passed"))
                    }
                }
                Err(e) => Err(format!("{e}")),
            },
            Err(e) => Err(format!("{:?}", e)),
        };

        Ok(TestReport::new(
            self.package,
            self.get_id(),
            self.get_version(),
            self.config.compiler_backend.to_string(),
            start - time::Instant::now(),
            outcome,
        ))
    }

    async fn is_to_test(&self) -> bool {
        let pkg = self.package;

        let out_dir = Argus::get_path(self.config.clone(), self.package).await;
        let test_results_path = out_dir.join(format!(
            "result-{}-{}--{}-{}.json",
            self.get_id(),
            self.get_version(),
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
