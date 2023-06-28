use std::{
    collections::{BTreeMap, VecDeque},
    fs::File,
    path::{Path, PathBuf},
};

use anyhow::{Context, Error};
use semver::Version;

use crate::runtime::resolver::{PackageSpecifier, PackageSummary, QueryError, Source};

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
    #[tracing::instrument(level = "debug", skip_all, fields(%package))]
    async fn query(&self, package: &PackageSpecifier) -> Result<Vec<PackageSummary>, QueryError> {
        match package {
            PackageSpecifier::Registry { full_name, version } => {
                match self.packages.get(full_name) {
                    Some(summaries) => {
                        let matches: Vec<_> = summaries
                            .iter()
                            .filter(|summary| version.matches(&summary.pkg.version))
                            .cloned()
                            .collect();

                        tracing::debug!(
                            matches = ?matches
                                .iter()
                                .map(|summary| summary.package_id().to_string())
                                .collect::<Vec<_>>(),
                        );

                        if matches.is_empty() {
                            return Err(QueryError::NoMatches {
                                archived_versions: Vec::new(),
                            });
                        }

                        Ok(matches)
                    }
                    None => Err(QueryError::NotFound),
                }
            }
            PackageSpecifier::Url(_) | PackageSpecifier::Path(_) => Err(QueryError::Unsupported),
        }
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use crate::runtime::resolver::{
        inputs::{DistributionInfo, FileSystemMapping, PackageInfo},
        Dependency, WebcHash,
    };

    use super::*;

    const PYTHON: &[u8] = include_bytes!("../../../../c-api/examples/assets/python-0.1.0.wasmer");
    const COREUTILS_16: &[u8] = include_bytes!("../../../../../tests/integration/cli/tests/webc/coreutils-1.0.16-e27dbb4f-2ef2-4b44-b46a-ddd86497c6d7.webc");
    const COREUTILS_11: &[u8] = include_bytes!("../../../../../tests/integration/cli/tests/webc/coreutils-1.0.11-9d7746ca-694f-11ed-b932-dead3543c068.webc");
    const BASH: &[u8] = include_bytes!("../../../../../tests/integration/cli/tests/webc/bash-1.0.16-f097441a-a80b-4e0d-87d7-684918ef4bb6.webc");

    #[test]
    fn load_a_directory_tree() {
        let temp = TempDir::new().unwrap();
        std::fs::write(temp.path().join("python-0.1.0.webc"), PYTHON).unwrap();
        std::fs::write(temp.path().join("coreutils-1.0.16.webc"), COREUTILS_16).unwrap();
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
                    version: "1.0.16".parse().unwrap(),
                    dependencies: vec![Dependency {
                        alias: "coreutils".to_string(),
                        pkg: "sharrattj/coreutils@^1.0.16".parse().unwrap()
                    }],
                    commands: vec![crate::runtime::resolver::Command {
                        name: "bash".to_string(),
                    }],
                    entrypoint: Some("bash".to_string()),
                    filesystem: vec![FileSystemMapping {
                        volume_name: "atom".to_string(),
                        mount_path: "/".to_string(),
                        original_path: "/".to_string(),
                        dependency_name: None,
                    }],
                },
                dist: DistributionInfo {
                    webc: crate::runtime::resolver::utils::url_from_file_path(
                        bash.canonicalize().unwrap()
                    )
                    .unwrap(),
                    webc_sha256: WebcHash::from_bytes([
                        161, 101, 23, 194, 244, 92, 186, 213, 143, 33, 200, 128, 238, 23, 185, 174,
                        180, 195, 144, 145, 78, 17, 227, 159, 118, 64, 83, 153, 0, 205, 253, 215,
                    ]),
                },
            }
        );
    }
}
