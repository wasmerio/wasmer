use std::path::{Path, PathBuf};

use anyhow::{Context, Error};
use semver::Version;
use wasmer_config::package::{
    NamedPackageId, NamedPackageIdent, PackageHash, PackageId, PackageIdent, PackageSource,
};

use crate::runtime::resolver::{PackageSummary, QueryError, Source, WebcHash};

/// A [`Source`] backed by a directory tree laid out like a registry:
/// `<root>/<namespace>/<name>/<version>.webc`, or `<root>/<name>/<version>.webc`
/// for un-namespaced packages.
///
/// Queries are answered from the layout alone: a named query lists the queried
/// package's directory and picks versions off the file names, so only the webcs
/// that match the version constraint are ever opened. A large tree costs no
/// more than the packages actually resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalRegistrySource {
    root: PathBuf,
}

impl LocalRegistrySource {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, Error> {
        let root = root.into();
        // Fail fast if the directory doesn't exist rather than
        // leaving all packages to resolve to 'not found'
        anyhow::ensure!(
            root.is_dir(),
            "package directory does not exist: \"{}\"",
            root.display()
        );
        Ok(LocalRegistrySource { root })
    }

    fn query_named(
        &self,
        named: &NamedPackageIdent,
        query: &PackageSource,
    ) -> Result<Vec<PackageSummary>, QueryError> {
        let full_name = named.full_name();
        let dir = match package_dir(&self.root, &full_name) {
            // Either the name can't exist in this layout or nothing is
            // published under it.
            Some(dir) if dir.is_dir() => dir,
            _ => {
                return Err(QueryError::NotFound {
                    query: query.clone(),
                });
            }
        };

        let constraint = named.version_or_default();
        let mut matches =
            published_versions(&dir).map_err(|error| QueryError::new_other(error, query))?;
        matches.retain(|(version, _)| constraint.matches(version));
        matches.sort_by(|(left, _), (right, _)| left.cmp(right));

        if matches.is_empty() {
            return Err(QueryError::NoMatches {
                query: query.clone(),
                archived_versions: Vec::new(),
            });
        }

        // Only now is any webc opened, and only the matching ones.
        matches
            .into_iter()
            .map(|(version, path)| {
                let id = NamedPackageId {
                    full_name: full_name.clone(),
                    version,
                };
                load_summary(&path, Some(PackageId::Named(id)))
            })
            .collect::<Result<Vec<_>, Error>>()
            .map_err(|error| QueryError::new_other(error, query))
    }

    fn query_hash(
        &self,
        hash: &PackageHash,
        query: &PackageSource,
    ) -> Result<Vec<PackageSummary>, QueryError> {
        match self
            .find_by_hash(hash)
            .map_err(|error| QueryError::new_other(error, query))?
        {
            Some(summary) => Ok(vec![summary]),
            None => Err(QueryError::NotFound {
                query: query.clone(),
            }),
        }
    }

    /// Walk the tree for a webc with this hash, stopping at the first match.
    /// This is the one query shape the layout can't index; a `.sha256` sidecar
    /// rules a file in or out without opening it, anything else must be hashed.
    fn find_by_hash(&self, hash: &PackageHash) -> Result<Option<PackageSummary>, Error> {
        let Some(expected) = hash.as_sha256().map(|digest| digest.to_string()) else {
            return Ok(None);
        };

        let mut stack = vec![self.root.clone()];
        while let Some(current) = stack.pop() {
            if current.is_dir() {
                for entry in std::fs::read_dir(&current)
                    .with_context(|| format!("Unable to read \"{}\"", current.display()))?
                {
                    stack.push(entry?.path());
                }
                continue;
            }
            if current.extension().and_then(|e| e.to_str()) != Some("webc") {
                continue;
            }
            let matches = match read_sha256_sibling(&current) {
                Some(claimed) => claimed
                    .strip_prefix("sha256:")
                    .unwrap_or(&claimed)
                    .eq_ignore_ascii_case(&expected),
                None => WebcHash::for_file(&current)
                    .with_context(|| format!("Unable to hash \"{}\"", current.display()))?
                    .as_hex()
                    .eq_ignore_ascii_case(&expected),
            };
            if !matches {
                continue;
            }

            let id = id_from_path(&self.root, &current).map(PackageId::Named);
            let summary = load_summary(&current, id)?;
            // A sidecar may match the query and still misdescribe the file.
            verify_sha256(&current, &summary.dist.webc_sha256, &expected)?;
            return Ok(Some(summary));
        }

        Ok(None)
    }
}

#[async_trait::async_trait]
impl Source for LocalRegistrySource {
    #[tracing::instrument(level = "debug", skip_all, fields(%package))]
    async fn query(&self, package: &PackageSource) -> Result<Vec<PackageSummary>, QueryError> {
        match package {
            PackageSource::Ident(PackageIdent::Named(named)) => {
                crate::block_in_place(|| self.query_named(named, package))
            }
            PackageSource::Ident(PackageIdent::Hash(hash)) => {
                crate::block_in_place(|| self.query_hash(hash, package))
            }
            PackageSource::Url(_) | PackageSource::Path(_) => Err(QueryError::Unsupported {
                query: package.clone(),
            }),
        }
    }
}

/// The directory holding a package's published versions: the full name's
/// components (namespace, then name) become path components under `root`.
/// `None` for names the layout can't hold (empty or path-like components),
/// rather than letting them escape the root.
fn package_dir(root: &Path, full_name: &str) -> Option<PathBuf> {
    let mut dir = root.to_path_buf();
    for part in full_name.split('/') {
        if part.is_empty() || part == "." || part == ".." || part.contains(std::path::is_separator)
        {
            return None;
        }
        dir.push(part);
    }
    Some(dir)
}

/// The versions published for one package: its directory's `<version>.webc`
/// files, read from the names alone. No webc is opened.
fn published_versions(dir: &Path) -> Result<Vec<(Version, PathBuf)>, Error> {
    let mut versions = Vec::new();
    for entry in dir
        .read_dir()
        .with_context(|| format!("Unable to read \"{}\"", dir.display()))?
    {
        let path = entry?.path();
        let Some(version) = path
            .file_name()
            .and_then(|f| f.to_str())
            .and_then(|f| f.strip_suffix(".webc"))
            .and_then(|stem| stem.parse().ok())
        else {
            continue;
        };
        versions.push((version, path));
    }
    Ok(versions)
}

/// The package id encoded by a webc's location under `root`
/// (`<namespace>/<name>/<version>.webc` or `<name>/<version>.webc`), if it
/// fits the layout. Used where a walk finds a file and its id must be derived
/// backwards; named lookups go the other way via [`package_dir`].
fn id_from_path(root: &Path, webc: &Path) -> Option<NamedPackageId> {
    let rel = webc.strip_prefix(root).ok()?;
    let parts = rel.iter().map(|p| p.to_str()).collect::<Option<Vec<_>>>()?;
    let (full_name, file) = match parts.as_slice() {
        [namespace, name, file] => (format!("{namespace}/{name}"), *file),
        [name, file] => (name.to_string(), *file),
        _ => return None,
    };
    let version = file.strip_suffix(".webc")?.parse().ok()?;
    Some(NamedPackageId { full_name, version })
}

/// Open one matched webc and build its summary. The layout's id (when given)
/// wins over the manifest's; a `<stem>.sha256` sibling, if present, must match
/// the content.
fn load_summary(webc: &Path, id: Option<PackageId>) -> Result<PackageSummary, Error> {
    let mut summary = PackageSummary::from_webc_file(webc)
        .with_context(|| format!("Unable to load \"{}\"", webc.display()))?;
    if let Some(expected) = read_sha256_sibling(webc) {
        verify_sha256(webc, &summary.dist.webc_sha256, &expected)?;
    }
    if let Some(id) = id {
        summary.pkg.id = id;
    }
    Ok(summary)
}

/// The trimmed digest of an optional `<stem>.sha256` sibling, if present.
fn read_sha256_sibling(webc: &Path) -> Option<String> {
    let contents = std::fs::read_to_string(webc.with_extension("sha256")).ok()?;
    Some(contents.trim().to_string())
}

/// Error if `actual` doesn't match `expected` (hex, an optional `sha256:`
/// prefix allowed).
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

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    const COREUTILS_16: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../wasmer-test-files/integration/webc/coreutils-1.0.16-e27dbb4f-2ef2-4b44-b46a-ddd86497c6d7.webc"
    ));
    const COREUTILS_11: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../wasmer-test-files/integration/webc/coreutils-1.0.11-9d7746ca-694f-11ed-b932-dead3543c068.webc"
    ));

    fn registry(files: &[(&str, &[u8])]) -> TempDir {
        let temp = TempDir::new().unwrap();
        for (rel, bytes) in files {
            let path = temp.path().join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
        temp
    }

    fn named(id: &str, version: &str) -> PackageId {
        PackageId::Named(NamedPackageId::try_new(id, version).unwrap())
    }

    #[test]
    fn new_rejects_a_missing_directory() {
        let temp = TempDir::new().unwrap();
        assert!(LocalRegistrySource::new(temp.path().join("nope")).is_err());
        assert!(LocalRegistrySource::new(temp.path()).is_ok());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn named_query_ids_come_from_the_layout() {
        // COREUTILS_16's manifest is "sharrattj/coreutils@1.0.16"; the layout
        // must win, proving resolution keys off the path, not the artifact.
        let temp = registry(&[("acme/cutils/9.9.9.webc", COREUTILS_16)]);
        let source = LocalRegistrySource::new(temp.path()).unwrap();

        let summaries = source.query(&"acme/cutils".parse().unwrap()).await.unwrap();

        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].pkg.id, named("acme/cutils", "9.9.9"));
        // The metadata itself still comes from the webc.
        assert!(!summaries[0].pkg.commands.is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn only_matching_versions_are_opened() {
        // 0.1.0 is garbage; a constraint that rules it out never reads it.
        let temp = registry(&[
            ("acme/cutils/0.1.0.webc", b"not a webc".as_slice()),
            ("acme/cutils/9.9.9.webc", COREUTILS_16),
        ]);
        let source = LocalRegistrySource::new(temp.path()).unwrap();

        let summaries = source
            .query(&"acme/cutils@9.9.9".parse().unwrap())
            .await
            .unwrap();
        assert_eq!(summaries[0].pkg.id, named("acme/cutils", "9.9.9"));

        // ...while a constraint that pulls it in surfaces the corruption.
        assert!(matches!(
            source.query(&"acme/cutils".parse().unwrap()).await,
            Err(QueryError::Other { .. })
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn version_constraints_filter_the_listing() {
        let temp = registry(&[
            ("sharrattj/coreutils/1.0.11.webc", COREUTILS_11),
            ("sharrattj/coreutils/1.0.16.webc", COREUTILS_16),
        ]);
        let source = LocalRegistrySource::new(temp.path()).unwrap();

        let all = source
            .query(&"sharrattj/coreutils".parse().unwrap())
            .await
            .unwrap();
        assert_eq!(
            all.iter().map(|s| s.pkg.id.clone()).collect::<Vec<_>>(),
            [
                named("sharrattj/coreutils", "1.0.11"),
                named("sharrattj/coreutils", "1.0.16"),
            ]
        );

        let pinned = source
            .query(&"sharrattj/coreutils@^1.0.16".parse().unwrap())
            .await
            .unwrap();
        assert_eq!(pinned.len(), 1);
        assert_eq!(pinned[0].pkg.id, named("sharrattj/coreutils", "1.0.16"));

        assert!(matches!(
            source.query(&"sharrattj/unknown".parse().unwrap()).await,
            Err(QueryError::NotFound { .. })
        ));
        assert!(matches!(
            source
                .query(&"sharrattj/coreutils@^2".parse().unwrap())
                .await,
            Err(QueryError::NoMatches { .. })
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn unnamespaced_packages_sit_one_level_up() {
        let temp = registry(&[("cutils/1.0.0.webc", COREUTILS_16)]);
        let source = LocalRegistrySource::new(temp.path()).unwrap();

        let summaries = source.query(&"cutils".parse().unwrap()).await.unwrap();

        assert_eq!(summaries[0].pkg.id, named("cutils", "1.0.0"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn sha256_sidecar_is_verified() {
        let temp = registry(&[("acme/cutils/9.9.9.webc", COREUTILS_16)]);
        let sidecar = temp.path().join("acme/cutils/9.9.9.sha256");
        let source = LocalRegistrySource::new(temp.path()).unwrap();
        let query: PackageSource = "acme/cutils".parse().unwrap();

        std::fs::write(&sidecar, WebcHash::sha256(COREUTILS_16).as_hex()).unwrap();
        assert!(source.query(&query).await.is_ok());

        std::fs::write(&sidecar, "deadbeef").unwrap();
        assert!(matches!(
            source.query(&query).await,
            Err(QueryError::Other { .. })
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn hash_queries_walk_the_tree() {
        let temp = registry(&[
            ("acme/cutils/9.9.9.webc", COREUTILS_16),
            ("other/pkg/1.0.0.webc", COREUTILS_11),
        ]);
        let source = LocalRegistrySource::new(temp.path()).unwrap();
        let hash = |bytes| PackageHash::from_sha256_bytes(WebcHash::sha256(bytes).as_bytes());
        let query = |hash| PackageSource::Ident(PackageIdent::Hash(hash));

        let summaries = source.query(&query(hash(COREUTILS_16))).await.unwrap();
        assert_eq!(summaries[0].pkg.id, named("acme/cutils", "9.9.9"));

        assert!(matches!(
            source
                .query(&query(PackageHash::from_sha256_bytes([0; 32])))
                .await,
            Err(QueryError::NotFound { .. })
        ));
    }

    #[test]
    fn parse_path_ids() {
        let root = Path::new("/pkgs");
        let id = |p: &str| id_from_path(root, &root.join(p)).map(|id| id.to_string());

        assert_eq!(id("ns/name/1.2.3.webc").as_deref(), Some("ns/name@1.2.3"));
        assert_eq!(id("name/1.2.3.webc").as_deref(), Some("name@1.2.3"));
        assert_eq!(
            id("ns/name/1.0.0-rc.1.webc").as_deref(),
            Some("ns/name@1.0.0-rc.1")
        );
        // Doesn't fit the layout -> None, so a walk keeps the manifest id.
        assert_eq!(id("ns/name/notaversion.webc"), None); // bad semver
        assert_eq!(id("too/deep/ns/name/1.0.0.webc"), None); // wrong depth
        assert_eq!(id("flat.webc"), None); // no version directory
    }

    #[test]
    fn package_dirs_stay_under_the_root() {
        let root = Path::new("/pkgs");

        assert_eq!(
            package_dir(root, "ns/name"),
            Some(PathBuf::from("/pkgs/ns/name"))
        );
        assert_eq!(package_dir(root, "name"), Some(PathBuf::from("/pkgs/name")));
        assert_eq!(package_dir(root, "ns/.."), None);
        assert_eq!(package_dir(root, ""), None);
    }
}
