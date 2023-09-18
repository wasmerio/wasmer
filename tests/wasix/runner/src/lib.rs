use std::path::{Path, PathBuf};

use anyhow::{Context, Error};
use cargo_metadata::{CargoOpt, MetadataCommand, Package};
use libtest_mimic::Trial;
use once_cell::sync::Lazy;
use wasmer::Engine;

#[derive(Debug, Default, Clone)]
pub struct Resolver {
    /// The test suite crate.
    suite: Option<PathBuf>,
    /// An optional prefix that will be added to the start of each test's name.
    prefix: Option<String>,
    features: Vec<CargoOpt>,
}

impl Resolver {
    pub fn new() -> Self {
        Resolver::default()
    }

    pub fn features(&mut self, opt: CargoOpt) -> &mut Self {
        self.features.push(opt);
        self
    }

    pub fn with_prefix(&mut self, prefix: impl Into<String>) -> &mut Self {
        self.prefix = Some(prefix.into());
        self
    }

    pub fn with_suite(&mut self, suite: impl Into<PathBuf>) -> &mut Self {
        self.suite = Some(suite.into());
        self
    }

    pub fn resolve(&self, engine: Engine) -> Result<Vec<Trial>, Error> {
        let suite = self.discover()?;

        panic!("{suite:?}");

        Ok(Vec::new())
    }

    fn discover(&self) -> Result<Suite, Error> {
        let mut suite = self
            .suite
            .as_deref()
            .unwrap_or(&DEFAULT_SUITE)
            .to_path_buf();

        if suite.file_name().and_then(|f| f.to_str()) != Some("Cargo.toml") {
            suite.push("Cargo.toml");
        }

        let mut cmd = MetadataCommand::new();

        cmd.manifest_path(suite).no_deps();

        for opt in &self.features {
            cmd.features(opt.clone());
        }

        let metadata = cmd.exec()?;
        let Package { targets, .. } = metadata
            .root_package()
            .context("Unable to determine the suite package")?;

        let bins: Vec<_> = targets
            .iter()
            .filter(|target| target.kind.iter().any(|kind| kind == "bin"))
            .map(|target| target.name.clone())
            .collect();

        Ok(Suite {
            target_dir: metadata.target_directory.into(),
            bins,
        })
    }
}

#[derive(Debug, PartialEq)]
struct Suite {
    target_dir: PathBuf,
    bins: Vec<String>,
}

static DEFAULT_SUITE: Lazy<PathBuf> =
    Lazy::new(|| project_root().join("tests").join("wasix").join("suite"));

fn project_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .find(|p| p.join(".git").is_dir())
        .expect("Unable to find the crate root")
}
