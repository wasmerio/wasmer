use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
};

use anyhow::{Context, Error};
use wasmer_config::package::{NamedPackageId, PackageHash, PackageId, PackageIdent, PackageSource};

use crate::runtime::resolver::{PackageSummary, QueryError, Source, WebcHash};

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

    /// Constructor wrapper over [`Self::add_packages`].
    pub fn from_directory_tree(dir: impl Into<PathBuf>) -> Result<Self, Error> {
        let mut source = InMemorySource::default();
        source.add_packages(dir.into())?;
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

    /// Load every webc under `path` (a single file or a directory tree). `path`
    /// is the location-scheme root passed to [`Self::add_one`].
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
            } else if current.extension().and_then(|e| e.to_str()) == Some("webc") {
                self.add_one(root, &current)?;
            }
        }
        Ok(())
    }

    /// Load one webc. Its id comes from `id_from_path(root, webc)` (swap that call
    /// for `id_from_filename`/`id_from_sidecar` to change scheme), else the
    /// manifest id. Checks an optional sha256. Unreadable webcs are skipped.
    fn add_one(&mut self, root: &Path, webc: &Path) -> Result<(), Error> {
        let mut summary = match PackageSummary::from_webc_file(webc) {
            Ok(summary) => summary,
            Err(e) if webc == root => {
                return Err(e).with_context(|| format!("Unable to load \"{}\"", webc.display()));
            }
            Err(e) => {
                tracing::warn!(path=%webc.display(), error=&*e, "Skipping unreadable webc");
                return Ok(());
            }
        };
        if let Some(identity) = id_from_path(root, webc)? {
            if let Some(expected) = &identity.expected_sha256 {
                verify_sha256(webc, &summary.dist.webc_sha256, expected)?;
            }
            summary.pkg.id = identity.id;
        }
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

// Identity schemes. A built webc is anonymous (name stripped on build), so its id
// comes from the file location or a sidecar. `add_one` calls one of these; switch
// scheme by switching which. Each returns the id and an optional sha256 to verify,
// or `None` to fall back to the manifest id.

/// A package id and an optional sha256 (hex) to verify the webc against.
struct ResolvedIdentity {
    id: PackageId,
    expected_sha256: Option<String>,
}

/// christoph's scheme (Edge convention): `<namespace>--<name>@<version>.webc`, or
/// unnamespaced `<name>@<version>.webc`. `root` is unused.
#[allow(dead_code)] // inactive scheme; swap into `add_one` to use.
fn id_from_filename(_root: &Path, webc: &Path) -> Result<Option<ResolvedIdentity>, Error> {
    let Some((name, version)) = webc
        .file_name()
        .and_then(|f| f.to_str())
        .and_then(|f| f.strip_suffix(".webc"))
        .and_then(|stem| stem.rsplit_once('@'))
    else {
        return Ok(None);
    };
    let full_name = match name.split_once("--") {
        Some((namespace, name)) => format!("{namespace}/{name}"),
        None => name.to_string(),
    };
    let Ok(version) = version.parse() else {
        return Ok(None);
    };
    Ok(Some(ResolvedIdentity {
        id: PackageId::Named(NamedPackageId { full_name, version }),
        expected_sha256: read_sha256_sibling(webc),
    }))
}

/// nikolas's scheme: `<namespace>/<name>/<version>.webc` (or `<name>/<version>
/// .webc`) relative to `root`. Each field is a path component, nothing to escape.
fn id_from_path(root: &Path, webc: &Path) -> Result<Option<ResolvedIdentity>, Error> {
    let Some(parts) = webc
        .strip_prefix(root)
        .ok()
        .and_then(|rel| rel.iter().map(|p| p.to_str()).collect::<Option<Vec<_>>>())
    else {
        return Ok(None);
    };
    let (full_name, file) = match parts.as_slice() {
        [namespace, name, file] => (format!("{namespace}/{name}"), *file),
        [name, file] => (name.to_string(), *file),
        _ => return Ok(None),
    };
    let Some(Ok(version)) = file.strip_suffix(".webc").map(str::parse) else {
        return Ok(None);
    };
    Ok(Some(ResolvedIdentity {
        id: PackageId::Named(NamedPackageId { full_name, version }),
        expected_sha256: read_sha256_sibling(webc),
    }))
}

/// The `<stem>.webcm` sidecar's metadata.
#[allow(dead_code)] // inactive scheme; swap into `add_one` to use.
#[derive(serde::Deserialize)]
struct Webcm {
    name: String,
    version: String,
    #[serde(rename = "package-hash")]
    package_hash: Option<String>,
}

/// arshia's scheme: a `<stem>.webcm` TOML sidecar (name/version/hash) beside the
/// webc. `root` is unused. Absent gives `None`; malformed is an error.
#[allow(dead_code)] // inactive scheme; swap into `add_one` to use.
fn id_from_sidecar(_root: &Path, webc: &Path) -> Result<Option<ResolvedIdentity>, Error> {
    let sidecar = webc.with_extension("webcm");
    if !sidecar.is_file() {
        return Ok(None);
    }
    let contents = std::fs::read_to_string(&sidecar)
        .with_context(|| format!("Unable to read \"{}\"", sidecar.display()))?;
    let webcm: Webcm = toml::from_str(&contents)
        .with_context(|| format!("Invalid webcm \"{}\"", sidecar.display()))?;
    Ok(Some(ResolvedIdentity {
        id: PackageId::Named(NamedPackageId {
            full_name: webcm.name,
            version: webcm.version.parse().context("Invalid webcm version")?,
        }),
        expected_sha256: webcm.package_hash,
    }))
}

/// The trimmed digest of an optional `<stem>.sha256` sibling, if present.
fn read_sha256_sibling(webc: &Path) -> Option<String> {
    let contents = std::fs::read_to_string(webc.with_extension("sha256")).ok()?;
    Some(contents.trim().to_string())
}

/// Error if `actual` doesn't match `expected` (hex, an optional `sha256:` prefix
/// allowed).
fn verify_sha256(webc: &Path, actual: &WebcHash, expected: &str) -> Result<(), Error> {
    let expected = expected.trim();
    let expected = expected.strip_prefix("sha256:").unwrap_or(expected);
    if !expected.eq_ignore_ascii_case(&actual.as_hex()) {
        anyhow::bail!(
            "sha256 mismatch for \"{}\": expected {expected}, file hashes to {}",
            webc.display(),
            actual.as_hex(),
        );
    }
    Ok(())
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

    #[test]
    fn parse_path_ids() {
        let root = Path::new("/pkgs");
        let id = |p: &str| {
            id_from_path(root, &root.join(p))
                .unwrap()
                .map(|ri| ri.id.to_string())
        };

        assert_eq!(id("ns/name/1.2.3.webc").as_deref(), Some("ns/name@1.2.3"));
        assert_eq!(id("name/1.2.3.webc").as_deref(), Some("name@1.2.3"));
        assert_eq!(
            id("ns/name/1.0.0-rc.1.webc").as_deref(),
            Some("ns/name@1.0.0-rc.1")
        );
        // doesn't fit the layout -> None, so the caller keeps the manifest id.
        assert_eq!(id("ns/name/notaversion.webc"), None); // bad semver
        assert_eq!(id("too/deep/ns/name/1.0.0.webc"), None); // wrong depth
        assert_eq!(id("flat.webc"), None); // no version directory
    }

    #[test]
    fn parse_filename_ids() {
        let id = |s: &str| {
            id_from_filename(Path::new(""), Path::new(s))
                .unwrap()
                .map(|ri| ri.id.to_string())
        };

        assert_eq!(id("ns--name@1.2.3.webc").as_deref(), Some("ns/name@1.2.3"));
        assert_eq!(id("name@1.2.3.webc").as_deref(), Some("name@1.2.3"));
        assert_eq!(id("ns--name@notaversion.webc"), None); // bad semver
        assert_eq!(id(&format!("{}.webc", "a".repeat(64))), None); // hash-named
    }

    #[test]
    fn add_packages_names_from_layout() {
        let temp = TempDir::new().unwrap();
        let dir = temp.path().join("acme").join("cutils");
        std::fs::create_dir_all(&dir).unwrap();
        // COREUTILS_16's manifest is "sharrattj/coreutils@1.0.16"; the layout
        // must win, proving resolution keys off the path, not the artifact.
        std::fs::write(dir.join("9.9.9.webc"), COREUTILS_16).unwrap();

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
    }

    #[test]
    fn add_packages_single_file_falls_back_to_manifest() {
        // A lone file has no layout, so the path scheme keeps its manifest id.
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
    fn add_packages_rejects_bad_sha256_sidecar() {
        let temp = TempDir::new().unwrap();
        let dir = temp.path().join("acme").join("cutils");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("9.9.9.webc"), COREUTILS_16).unwrap();
        std::fs::write(dir.join("9.9.9.sha256"), "deadbeef").unwrap();

        let mut source = InMemorySource::new();
        assert!(source.add_packages(temp.path()).is_err());
    }

    #[test]
    fn sidecar_identity() {
        // add_one uses the path scheme, so test the sidecar scheme directly.
        let temp = TempDir::new().unwrap();
        let webc = temp.path().join("pkg.webc");
        std::fs::write(&webc, COREUTILS_16).unwrap();
        let hash = WebcHash::sha256(COREUTILS_16).as_hex();
        std::fs::write(
            temp.path().join("pkg.webcm"),
            format!("name = \"acme/cutils\"\nversion = \"9.9.9\"\npackage-hash = \"{hash}\"\n"),
        )
        .unwrap();

        let identity = id_from_sidecar(temp.path(), &webc).unwrap().unwrap();
        assert_eq!(identity.id.to_string(), "acme/cutils@9.9.9");

        let actual = WebcHash::sha256(COREUTILS_16);
        verify_sha256(&webc, &actual, identity.expected_sha256.as_deref().unwrap()).unwrap();
        assert!(verify_sha256(&webc, &actual, "deadbeef").is_err());
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
