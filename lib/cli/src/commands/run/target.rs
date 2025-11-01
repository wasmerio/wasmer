use std::{
    fs::File,
    io::Read as _,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context as _, Error, bail};
use indicatif::ProgressBar;
use wasmer::Module;
use wasmer_types::ModuleHash;
use wasmer_wasix::{
    Runtime,
    bin_factory::BinaryPackage,
    runtime::{module_cache::HashedModuleData, task_manager::VirtualTaskManagerExt as _},
};
#[cfg(feature = "compiler")]
use wasmer_compiler::ArtifactBuild;

/// We've been given the path for a file... What does it contain and how should
/// that be run?
#[derive(Debug, Clone)]
pub enum TargetOnDisk {
    WebAssemblyBinary,
    Wat,
    LocalWebc,
    Artifact,
}

impl TargetOnDisk {
    pub fn from_file(path: &Path) -> Result<TargetOnDisk, Error> {
        // Normally the first couple hundred bytes is enough to figure
        // out what type of file this is.
        let mut buffer = [0_u8; 512];

        let mut f = File::open(path)
            .with_context(|| format!("Unable to open \"{}\" for reading", path.display()))?;
        let bytes_read = f.read(&mut buffer)?;

        let leading_bytes = &buffer[..bytes_read];

        if wasmer::is_wasm(leading_bytes) {
            return Ok(TargetOnDisk::WebAssemblyBinary);
        }

        if webc::detect(leading_bytes).is_ok() {
            return Ok(TargetOnDisk::LocalWebc);
        }

        #[cfg(feature = "compiler")]
        if ArtifactBuild::is_deserializable(leading_bytes) {
            return Ok(TargetOnDisk::Artifact);
        }

        // If we can't figure out the file type based on its content, fall back
        // to checking the extension.

        match path.extension().and_then(|s| s.to_str()) {
            Some("wat") => Ok(TargetOnDisk::Wat),
            Some("wasm") => Ok(TargetOnDisk::WebAssemblyBinary),
            Some("webc") => Ok(TargetOnDisk::LocalWebc),
            Some("wasmu") => Ok(TargetOnDisk::WebAssemblyBinary),
            _ => bail!("Unable to determine how to execute \"{}\"", path.display()),
        }
    }
}

#[derive(Debug, Clone)]
pub enum ExecutableTarget {
    WebAssembly {
        module: Module,
        module_hash: ModuleHash,
        path: PathBuf,
    },
    Package(Box<BinaryPackage>),
}

impl ExecutableTarget {
    /// Try to load a Wasmer package from a directory containing a `wasmer.toml`
    /// file.
    #[tracing::instrument(level = "debug", skip_all)]
    pub fn from_dir(
        dir: &Path,
        runtime: &Arc<dyn Runtime + Send + Sync>,
        pb: &ProgressBar,
    ) -> Result<Self, Error> {
        pb.set_message(format!("Loading \"{}\" into memory", dir.display()));
        pb.set_message("Resolving dependencies");
        let inner_runtime = runtime.clone();
        let pkg = runtime.task_manager().spawn_and_block_on({
            let path = dir.to_path_buf();

            async move { BinaryPackage::from_dir(&path, inner_runtime.as_ref()).await }
        })??;

        Ok(ExecutableTarget::Package(Box::new(pkg)))
    }

    /// Try to load a file into something that can be used to run it.
    #[tracing::instrument(level = "debug", skip_all)]
    pub fn from_file(
        path: &Path,
        runtime: &Arc<dyn Runtime + Send + Sync>,
        pb: &ProgressBar,
    ) -> Result<Self, Error> {
        pb.set_message(format!("Loading from \"{}\"", path.display()));

        match TargetOnDisk::from_file(path)? {
            TargetOnDisk::WebAssemblyBinary | TargetOnDisk::Wat => {
                let wasm = std::fs::read(path)?;
                let module_data = HashedModuleData::new(wasm);
                let module_hash = *module_data.hash();

                pb.set_message("Compiling to WebAssembly");
                let module = runtime
                    .load_hashed_module_sync(module_data, None)
                    .with_context(|| format!("Unable to compile \"{}\"", path.display()))?;

                Ok(ExecutableTarget::WebAssembly {
                    module,
                    module_hash,
                    path: path.to_path_buf(),
                })
            }
            TargetOnDisk::Artifact => {
                let engine = runtime.engine();
                pb.set_message("Deserializing pre-compiled WebAssembly module");
                let module = unsafe { Module::deserialize_from_file(&engine, path)? };

                let module_hash = module.info().hash.ok_or_else(|| {
                    anyhow::Error::msg("module hash is not present in the artifact")
                })?;

                Ok(ExecutableTarget::WebAssembly {
                    module,
                    module_hash,
                    path: path.to_path_buf(),
                })
            }
            TargetOnDisk::LocalWebc => {
                let container = wasmer_package::utils::from_disk(path)?;
                pb.set_message("Resolving dependencies");

                let inner_runtime = runtime.clone();
                let pkg = runtime.task_manager().spawn_and_block_on(async move {
                    BinaryPackage::from_webc(&container, inner_runtime.as_ref()).await
                })??;
                Ok(ExecutableTarget::Package(Box::new(pkg)))
            }
        }
    }
}
