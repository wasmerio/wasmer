use std::{
    collections::{BTreeMap, VecDeque},
    fs::File,
    path::{Path, PathBuf},
};

use anyhow::{Context, Error};
use semver::Version;

use crate::runtime::resolver::{PackageSpecifier, PackageSummary, Source};

/// A [`Source`] that tracks packages in memory.
///
/// Primarily used during testing.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct InMemorySource {
    packages: BTreeMap<String, Vec<PackageSummary>>,
}

impl InMemorySource {
    pub fn new() -> Self {
        InMemorySource::default()
    }

    /// Recursively walk a directory, adding all valid WEBC files to the source.
    pub fn from_directory_tree(dir: impl Into<PathBuf>) -> Result<Self, Error> {
        let mut source = InMemorySource::default();

        let mut to_check: VecDeque<PathBuf> = VecDeque::new();
        to_check.push_back(dir.into());

        fn process_entry(
            path: &Path,
            source: &mut InMemorySource,
            to_check: &mut VecDeque<PathBuf>,
        ) -> Result<(), Error> {
            let metadata = std::fs::metadata(path).context("Unable to get filesystem metadata")?;

            if metadata.is_dir() {
                for entry in path.read_dir().context("Unable to read the directory")? {
                    to_check.push_back(entry?.path());
                }
            } else if metadata.is_file() {
                let f = File::open(path).context("Unable to open the file")?;
                if webc::detect(f).is_ok() {
                    source
                        .add_webc(path)
                        .with_context(|| format!("Unable to load \"{}\"", path.display()))?;
                }
            }

            Ok(())
        }

        while let Some(path) = to_check.pop_front() {
            process_entry(&path, &mut source, &mut to_check)
                .with_context(|| format!("Unable to add entries from \"{}\"", path.display()))?;
        }

        Ok(source)
    }

    /// Add a new [`PackageSummary`] to the [`InMemorySource`].
    pub fn add(&mut self, summary: PackageSummary) {
        let summaries = self.packages.entry(summary.pkg.name.clone()).or_default();
        summaries.push(summary);
        summaries.sort_by(|left, right| left.pkg.version.cmp(&right.pkg.version));
        summaries.dedup_by(|left, right| left.pkg.version == right.pkg.version);
    }

    pub fn add_webc(&mut self, path: impl AsRef<Path>) -> Result<(), Error> {
        let summary = PackageSummary::from_webc_file(path)?;
        self.add(summary);

        Ok(())
    }

    pub fn packages(&self) -> &BTreeMap<String, Vec<PackageSummary>> {
        &self.packages
    }

    pub fn get(&self, package_name: &str, version: &Version) -> Option<&PackageSummary> {
        let summaries = self.packages.get(package_name)?;
        summaries.iter().find(|s| s.pkg.version == *version)
    }
}

#[async_trait::async_trait]
impl Source for InMemorySource {
    async fn query(&self, package: &PackageSpecifier) -> Result<Vec<PackageSummary>, Error> {
        match package {
            PackageSpecifier::Registry { full_name, version } => {
                match self.packages.get(full_name) {
                    Some(summaries) => Ok(summaries
                        .iter()
                        .filter(|summary| version.matches(&summary.pkg.version))
                        .cloned()
                        .collect()),
                    None => Ok(Vec::new()),
                }
            }
            PackageSpecifier::Url(_) | PackageSpecifier::Path(_) => Ok(Vec::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use crate::runtime::resolver::{
        inputs::{DistributionInfo, PackageInfo},
        Dependency,
    };

    use super::*;

    const PYTHON: &[u8] = include_bytes!("../../../../c-api/examples/assets/python-0.1.0.wasmer");
    const COREUTILS_14: &[u8] = include_bytes!("../../../../../tests/integration/cli/tests/webc/coreutils-1.0.14-076508e5-e704-463f-b467-f3d9658fc907.webc");
    const COREUTILS_11: &[u8] = include_bytes!("../../../../../tests/integration/cli/tests/webc/coreutils-1.0.11-9d7746ca-694f-11ed-b932-dead3543c068.webc");
    const BASH: &[u8] = include_bytes!("../../../../../tests/integration/cli/tests/webc/bash-1.0.12-0103d733-1afb-4a56-b0ef-0e124139e996.webc");

    #[test]
    fn load_a_directory_tree() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join("python-0.1.0.webc"), PYTHON).unwrap();
        std::fs::write(temp.path().join("coreutils-1.0.14.webc"), COREUTILS_14).unwrap();
        std::fs::write(temp.path().join("coreutils-1.0.11.webc"), COREUTILS_11).unwrap();
        let nested = temp.path().join("nested");
        std::fs::create_dir(&nested).unwrap();
        let bash = nested.join("bash-1.0.12.webc");
        std::fs::write(&bash, BASH).unwrap();

        let source = InMemorySource::from_directory_tree(temp.path()).unwrap();

        assert_eq!(
            source
                .packages
                .keys()
                .map(|k| k.as_str())
                .collect::<Vec<_>>(),
            ["python", "sharrattj/bash", "sharrattj/coreutils"]
        );
        assert_eq!(source.packages["sharrattj/coreutils"].len(), 2);
        assert_eq!(
            source.packages["sharrattj/bash"][0],
            PackageSummary {
                pkg: PackageInfo {
                    name: "sharrattj/bash".to_string(),
                    version: "1.0.12".parse().unwrap(),
                    dependencies: vec![Dependency {
                        alias: "coreutils".to_string(),
                        pkg: "sharrattj/coreutils@^1.0.11".parse().unwrap()
                    }],
                    commands: ["bash", "sh"]
                        .iter()
                        .map(|name| crate::runtime::resolver::Command {
                            name: name.to_string()
                        })
                        .collect(),
                    entrypoint: None,
                },
                dist: DistributionInfo {
                    webc: crate::runtime::resolver::polyfills::url_from_file_path(
                        bash.canonicalize().unwrap()
                    )
                    .unwrap(),
                    webc_sha256: [
                        7, 226, 190, 131, 173, 231, 130, 245, 207, 185, 51, 189, 86, 85, 222, 37,
                        27, 163, 170, 27, 25, 24, 211, 136, 186, 233, 174, 119, 66, 15, 134, 9
                    ]
                    .into(),
                },
            }
        );
    }
}
