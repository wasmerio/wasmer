use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    fs::File,
    path::{Path, PathBuf},
};

use anyhow::{Context, Error};
use wasmer_config::package::{NamedPackageId, PackageHash, PackageId, PackageIdent, PackageSource};

use crate::runtime::resolver::{PackageSummary, QueryError, Source};

/// A [`Source`] that tracks packages in memory.
///
/// Primarily used during testing.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct InMemorySource {
    named_packages: BTreeMap<String, Vec<NamedPackageSummary>>,
    hash_packages: HashMap<PackageHash, PackageSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NamedPackageSummary {
    ident: NamedPackageId,
    summary: PackageSummary,
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
    ///
    /// Named packages are also made accessible by their hash.
    pub fn add(&mut self, summary: PackageSummary) {
        match summary.pkg.id.clone() {
            PackageId::Named(ident) => {
                // Also add the package as a hashed package.
                let pkg_hash = PackageHash::Sha256(wasmer_config::hash::Sha256Hash(
                    summary.dist.webc_sha256.as_bytes(),
                ));
                self.hash_packages
                    .entry(pkg_hash)
                    .or_insert_with(|| summary.clone());

                // Add the named package.
                let summaries = self
                    .named_packages
                    .entry(ident.full_name.clone())
                    .or_default();
                summaries.push(NamedPackageSummary { ident, summary });
                summaries.sort_by(|left, right| left.ident.version.cmp(&right.ident.version));
                summaries.dedup_by(|left, right| left.ident.version == right.ident.version);
            }
            PackageId::Hash(hash) => {
                self.hash_packages.insert(hash, summary);
            }
        }
    }

    pub fn add_webc(&mut self, path: impl AsRef<Path>) -> Result<(), Error> {
        let summary = PackageSummary::from_webc_file(path)?;
        self.add(summary);

        Ok(())
    }

    pub fn get(&self, id: &PackageId) -> Option<&PackageSummary> {
        match id {
            PackageId::Named(ident) => {
                self.named_packages
                    .get(&ident.full_name)
                    .and_then(|summaries| {
                        summaries
                            .iter()
                            .find(|s| s.ident.version == ident.version)
                            .map(|s| &s.summary)
                    })
            }
            PackageId::Hash(hash) => self.hash_packages.get(hash),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.named_packages.is_empty() && self.hash_packages.is_empty()
    }

    /// Returns the number of packages in the source.
    pub fn len(&self) -> usize {
        // Only need to count the hash packages,
        // as the named packages are also always added as hashed.
        self.hash_packages.len()
    }
}

#[async_trait::async_trait]
impl Source for InMemorySource {
    #[tracing::instrument(level = "debug", skip_all, fields(%package))]
    async fn query(&self, package: &PackageSource) -> Result<Vec<PackageSummary>, QueryError> {
        match package {
            PackageSource::Ident(PackageIdent::Named(named)) => {
                match self.named_packages.get(&named.full_name()) {
                    Some(summaries) => {
                        let matches: Vec<_> = summaries
                            .iter()
                            .filter(|summary| {
                                named.version_or_default().matches(&summary.ident.version)
                            })
                            .map(|n| n.summary.clone())
                            .collect();

                        tracing::trace!(
                            matches = ?matches
                                .iter()
                                .map(|summary| summary.pkg.id.to_string())
                                .collect::<Vec<_>>(),
                            "package resolution matches",
                        );

                        if matches.is_empty() {
                            return Err(QueryError::NoMatches {
                                query: package.clone(),
                                archived_versions: Vec::new(),
                            });
                        }

                        Ok(matches)
                    }
                    None => Err(QueryError::NotFound {
                        query: package.clone(),
                    }),
                }
            }
            PackageSource::Ident(PackageIdent::Hash(hash)) => self
                .hash_packages
                .get(hash)
                .map(|x| vec![x.clone()])
                .ok_or_else(|| QueryError::NoMatches {
                    query: package.clone(),
                    archived_versions: Vec::new(),
                }),
            PackageSource::Url(_) | PackageSource::Path(_) => Err(QueryError::Unsupported {
                query: package.clone(),
            }),
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
                .named_packages
                .keys()
                .map(|k| k.as_str())
                .collect::<Vec<_>>(),
            ["python", "sharrattj/bash", "sharrattj/coreutils"]
        );
        assert_eq!(source.named_packages["sharrattj/coreutils"].len(), 2);
        assert_eq!(
            source.named_packages["sharrattj/bash"][0].summary,
            PackageSummary {
                pkg: PackageInfo {
                    id: PackageId::Named(
                        NamedPackageId::try_new("sharrattj/bash", "1.0.16").unwrap()
                    ),
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
                        original_path: Some("/".to_string()),
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
