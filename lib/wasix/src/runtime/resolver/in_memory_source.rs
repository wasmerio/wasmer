use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
};

use anyhow::{Context, Error};
use wasmer_config::package::{
    NamedPackageId, PackageHash, PackageId, PackageIdent, PackageSource, Webcm,
};

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

    /// Load every package under `path` (a `.webc`, a `.webcm`, or a directory
    /// tree of them), keying each by the identity in its [`Webcm`] sidecar, or
    /// by its own manifest when it has none.
    ///
    /// An invalid sidecar (unparsable, missing its paired `.webc`, or a hash
    /// mismatch) is an error, never silently skipped.
    pub fn add_packages(&mut self, path: impl AsRef<Path>) -> Result<(), Error> {
        let root = path.as_ref();
        // Error on a missing path instead of silently loading nothing.
        anyhow::ensure!(
            root.exists(),
            "package path does not exist: \"{}\"",
            root.display()
        );
        let mut stack = vec![root.to_path_buf()];
        while let Some(current) = stack.pop() {
            if current.is_dir() {
                for entry in std::fs::read_dir(&current)
                    .with_context(|| format!("Unable to read \"{}\"", current.display()))?
                {
                    stack.push(entry?.path());
                }
            } else if current.extension().and_then(|e| e.to_str()) == Some(Webcm::EXTENSION) {
                self.add_from_webcm(&current)?;
            } else if current.extension().and_then(|e| e.to_str()) == Some(Webcm::WEBC_EXTENSION) {
                let sidecar = Webcm::path_for_webc(&current);
                if !sidecar.is_file() {
                    self.add_bare_webc(root, &current)?;
                } else if current == root {
                    // The sidecar drives loading. During a directory walk it
                    // is its own entry; for a file root, chase it explicitly.
                    self.add_from_webcm(&sidecar)?;
                }
            }
        }
        Ok(())
    }

    /// Load the webc paired with the `.webcm` at `webcm_path` under the
    /// sidecar's identity, verifying its hash when the sidecar records one.
    fn add_from_webcm(&mut self, webcm_path: &Path) -> Result<(), Error> {
        let contents = std::fs::read_to_string(webcm_path)
            .with_context(|| format!("Unable to read \"{}\"", webcm_path.display()))?;
        let webcm: Webcm = contents
            .parse()
            .with_context(|| format!("Invalid webcm \"{}\"", webcm_path.display()))?;

        let webc = Webcm::require_paired_webc(webcm_path)?;
        let mut summary = PackageSummary::from_webc_file(&webc)
            .with_context(|| format!("Unable to load \"{}\"", webc.display()))?;

        if let Some(expected) = &webcm.package.hash {
            let actual = PackageHash::from_sha256_bytes(summary.dist.webc_sha256.as_bytes());
            expected
                .ensure_matches(&actual)
                .with_context(|| format!("for webc \"{}\"", webc.display()))?;
        }
        summary.pkg.id = PackageId::Named(webcm.id());
        self.add(summary);
        Ok(())
    }

    /// Load a webc that has no sidecar, keeping its manifest id. Unreadable
    /// webcs found during a directory walk are skipped.
    fn add_bare_webc(&mut self, root: &Path, webc: &Path) -> Result<(), Error> {
        match PackageSummary::from_webc_file(webc) {
            Ok(summary) => self.add(summary),
            Err(e) if webc == root => {
                return Err(e).with_context(|| format!("Unable to load \"{}\"", webc.display()));
            }
            Err(e) => {
                tracing::warn!(path=%webc.display(), error=&*e, "Skipping unreadable webc");
            }
        }
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
        Dependency, WebcHash,
        inputs::{DistributionInfo, FileSystemMapping, PackageInfo},
    };

    use super::*;

    const PYTHON: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../wasmer-test-files/examples/python-0.1.0.wasmer"
    ));
    const COREUTILS_16: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../wasmer-test-files/integration/webc/coreutils-1.0.16-e27dbb4f-2ef2-4b44-b46a-ddd86497c6d7.webc"
    ));
    const COREUTILS_11: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../wasmer-test-files/integration/webc/coreutils-1.0.11-9d7746ca-694f-11ed-b932-dead3543c068.webc"
    ));
    const BASH: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../wasmer-test-files/integration/webc/bash-1.0.16-f097441a-a80b-4e0d-87d7-684918ef4bb6.webc"
    ));

    /// Write a `.webcm` pairing with `webc`, recording COREUTILS_16's real
    /// hash unless `hash` overrides it.
    fn write_webcm(webc: &Path, name: &str, version: &str, hash: Option<&str>) {
        let hash = match hash {
            Some(hash) => hash.to_string(),
            None => format!("sha256:{}", WebcHash::sha256(COREUTILS_16).as_hex()),
        };
        std::fs::write(
            Webcm::path_for_webc(webc),
            format!("[package]\nname = \"{name}\"\nversion = \"{version}\"\nhash = \"{hash}\"\n"),
        )
        .unwrap();
    }

    #[test]
    fn add_packages_names_from_webcm() {
        let temp = TempDir::new().unwrap();
        let webc = temp.path().join("cutils.webc");
        // COREUTILS_16's own manifest is "sharrattj/coreutils@1.0.16"; the
        // sidecar must win, proving it is the authoritative identity.
        std::fs::write(&webc, COREUTILS_16).unwrap();
        write_webcm(&webc, "acme/cutils", "9.9.9", None);

        let mut source = InMemorySource::new();
        source.add_packages(temp.path()).unwrap();

        assert_eq!(
            source
                .named_packages
                .keys()
                .map(|k| k.as_str())
                .collect::<Vec<_>>(),
            ["acme/cutils"]
        );
        assert_eq!(
            source.named_packages["acme/cutils"][0].ident,
            NamedPackageId::try_new("acme/cutils", "9.9.9").unwrap()
        );
        // Not double-added under its manifest id by the directory walk.
        assert_eq!(source.len(), 1);
    }

    #[test]
    fn add_packages_accepts_webc_or_webcm_file_roots() {
        let temp = TempDir::new().unwrap();
        let webc = temp.path().join("cutils.webc");
        std::fs::write(&webc, COREUTILS_16).unwrap();
        write_webcm(&webc, "acme/cutils", "9.9.9", None);

        for root in [webc.clone(), Webcm::path_for_webc(&webc)] {
            let mut source = InMemorySource::new();
            source.add_packages(&root).unwrap();
            assert_eq!(
                source.named_packages["acme/cutils"][0].ident,
                NamedPackageId::try_new("acme/cutils", "9.9.9").unwrap(),
                "root: {}",
                root.display()
            );
        }
    }

    #[test]
    fn webcm_hash_is_optional() {
        let temp = TempDir::new().unwrap();
        let webc = temp.path().join("cutils.webc");
        std::fs::write(&webc, COREUTILS_16).unwrap();
        std::fs::write(
            Webcm::path_for_webc(&webc),
            "[package]\nname = \"acme/cutils\"\nversion = \"9.9.9\"\n",
        )
        .unwrap();

        let mut source = InMemorySource::new();
        source.add_packages(temp.path()).unwrap();

        assert!(source.named_packages.contains_key("acme/cutils"));
    }

    #[test]
    fn webcm_hash_mismatch_is_an_error() {
        let temp = TempDir::new().unwrap();
        let webc = temp.path().join("cutils.webc");
        std::fs::write(&webc, COREUTILS_16).unwrap();
        write_webcm(
            &webc,
            "acme/cutils",
            "9.9.9",
            Some(&format!("sha256:{}", "a".repeat(64))),
        );

        let mut source = InMemorySource::new();
        let err = source.add_packages(temp.path()).unwrap_err();
        assert!(format!("{err:#}").contains("hash mismatch"), "{err:#}");
    }

    #[test]
    fn orphaned_webcm_is_an_error() {
        let temp = TempDir::new().unwrap();
        std::fs::write(
            temp.path().join("cutils.webcm"),
            "[package]\nname = \"acme/cutils\"\nversion = \"9.9.9\"\n",
        )
        .unwrap();

        let mut source = InMemorySource::new();
        let err = source.add_packages(temp.path()).unwrap_err();
        assert!(format!("{err:#}").contains("no paired webc"), "{err:#}");
    }

    #[test]
    fn malformed_webcm_is_an_error() {
        let temp = TempDir::new().unwrap();
        let webc = temp.path().join("cutils.webc");
        std::fs::write(&webc, COREUTILS_16).unwrap();
        std::fs::write(Webcm::path_for_webc(&webc), "not webcm").unwrap();

        let mut source = InMemorySource::new();
        assert!(source.add_packages(temp.path()).is_err());
    }

    #[test]
    fn add_packages_single_file_falls_back_to_manifest() {
        // Without a sidecar, a webc keeps its manifest id.
        let temp = TempDir::new().unwrap();
        let file = temp.path().join("anything.webc");
        std::fs::write(&file, COREUTILS_16).unwrap();

        let mut source = InMemorySource::new();
        source.add_packages(&file).unwrap();

        assert_eq!(
            source.named_packages["sharrattj/coreutils"][0].ident,
            NamedPackageId::try_new("sharrattj/coreutils", "1.0.16").unwrap()
        );
    }

    #[test]
    fn add_packages_errors_on_missing_path() {
        let temp = TempDir::new().unwrap();
        let missing = temp.path().join("does-not-exist");

        let mut source = InMemorySource::new();
        assert!(source.add_packages(&missing).is_err());
    }

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

        let mut source = InMemorySource::new();
        source.add_packages(temp.path()).unwrap();

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
