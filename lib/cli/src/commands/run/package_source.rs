use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Error;
use indicatif::ProgressBar;
use wasmer_config::package::PackageSource;
use wasmer_wasix::{
    Runtime, bin_factory::BinaryPackage, runtime::task_manager::VirtualTaskManagerExt as _,
};

use super::ExecutableTarget;

/// CLI representation of a target to execute.
#[derive(Debug, Clone, PartialEq)]
pub enum CliPackageSource {
    /// A file on disk (`*.wasm`, `*.webc`, etc.).
    File(PathBuf),
    /// A directory containing a `wasmer.toml` file
    Dir(PathBuf),
    /// A package to be downloaded (a URL, package name, etc.)
    Package(PackageSource),
}

impl CliPackageSource {
    pub fn infer(s: &str) -> Result<CliPackageSource, Error> {
        let path = Path::new(s);
        if path.is_file() {
            return Ok(Self::File(path.to_path_buf()));
        } else if path.is_dir() {
            return Ok(Self::Dir(path.to_path_buf()));
        }

        if let Ok(pkg) = s.parse::<PackageSource>() {
            return Ok(Self::Package(pkg));
        }

        Err(anyhow::anyhow!(
            "Unable to resolve \"{s}\" as a URL, package name, or file on disk"
        ))
    }

    /// Try to resolve the [`PackageSource`] to an executable artifact.
    ///
    /// This will try to automatically download and cache any resources from the
    /// internet.
    #[tracing::instrument(level = "debug", skip_all)]
    pub fn resolve_target(
        &self,
        rt: &Arc<dyn Runtime + Send + Sync>,
        pb: &ProgressBar,
    ) -> Result<ExecutableTarget, Error> {
        match self {
            Self::File(path) => ExecutableTarget::from_file(path, rt, pb),
            Self::Dir(d) => ExecutableTarget::from_dir(d, rt, pb),
            Self::Package(pkg) => {
                pb.set_message("Loading from the registry");
                let inner_pck = pkg.clone();
                let inner_rt = rt.clone();
                let pkg = rt.task_manager().spawn_and_block_on(async move {
                    BinaryPackage::from_registry(&inner_pck, inner_rt.as_ref()).await
                })??;
                Ok(ExecutableTarget::Package(Box::new(pkg)))
            }
        }
    }
}

impl std::fmt::Display for CliPackageSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::File(path) | Self::Dir(path) => {
                write!(f, "{}", path.display())
            }
            Self::Package(p) => write!(f, "{p}"),
        }
    }
}
