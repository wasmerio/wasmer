use std::{
    collections::{BTreeMap, VecDeque},
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

use anyhow::{Context, Error};
use sha2::{Digest, Sha256};
use url::Url;
use webc::{
    metadata::{annotations::Wapm, UrlOrManifest},
    Container,
};

use crate::runtime::resolver::{PackageSpecifier, Source, SourceId, SourceKind, Summary};

use super::Dependency;

/// A [`Source`] that tracks packages in memory.
///
/// Primarily used during testing.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct InMemorySource {
    packages: BTreeMap<String, Vec<Summary>>,
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
                    let summary =
                        webc_summary(path, source.id()).context("Unable to load the summary")?;
                    source.insert(summary);
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

    /// Add a new [`Summary`] to the [`InMemorySource`].
    pub fn insert(&mut self, summary: Summary) {
        let summaries = self
            .packages
            .entry(summary.package_name.clone())
            .or_default();
        summaries.push(summary);
        summaries.sort_by(|left, right| left.version.cmp(&right.version));
        summaries.dedup_by(|left, right| left.version == right.version);
    }

    pub fn packages(&self) -> &BTreeMap<String, Vec<Summary>> {
        &self.packages
    }
}

#[async_trait::async_trait]
impl Source for InMemorySource {
    fn id(&self) -> SourceId {
        // FIXME: We need to have a proper SourceId here
        SourceId::new(
            SourceKind::LocalRegistry,
            Url::from_directory_path("/").unwrap(),
        )
    }

    async fn query(&self, package: &PackageSpecifier) -> Result<Vec<Summary>, Error> {
        match package {
            PackageSpecifier::Registry { full_name, version } => {
                match self.packages.get(full_name) {
                    Some(summaries) => Ok(summaries
                        .iter()
                        .filter(|summary| version.matches(&summary.version))
                        .cloned()
                        .collect()),
                    None => Ok(Vec::new()),
                }
            }
            PackageSpecifier::Url(_) | PackageSpecifier::Path(_) => Ok(Vec::new()),
        }
    }
}

fn webc_summary(path: &Path, source: SourceId) -> Result<Summary, Error> {
    let path = path.canonicalize()?;
    let container = Container::from_disk(&path)?;
    let manifest = container.manifest();

    let dependencies = manifest
        .use_map
        .iter()
        .map(|(alias, value)| {
            let pkg = url_or_manifest_to_specifier(value)?;
            Ok(Dependency {
                alias: alias.clone(),
                pkg,
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;

    let commands = manifest
        .commands
        .iter()
        .map(|(name, _value)| crate::runtime::resolver::Command {
            name: name.to_string(),
        })
        .collect();

    let Wapm { name, version, .. } = manifest
        .package_annotation("wapm")?
        .context("No \"wapm\" annotations found")?;

    let webc_sha256 = file_hash(&path)?;

    Ok(Summary {
        package_name: name,
        version: version.parse()?,
        webc: Url::from_file_path(path).expect("We've already canonicalized the path"),
        webc_sha256,
        dependencies,
        commands,
        source,
        entrypoint: manifest.entrypoint.clone(),
    })
}

fn url_or_manifest_to_specifier(value: &UrlOrManifest) -> Result<PackageSpecifier, Error> {
    match value {
        UrlOrManifest::Url(url) => Ok(PackageSpecifier::Url(url.clone())),
        UrlOrManifest::Manifest(manifest) => {
            if let Ok(Some(Wapm { name, version, .. })) = manifest.package_annotation("wapm") {
                let version = version.parse()?;
                return Ok(PackageSpecifier::Registry {
                    full_name: name,
                    version,
                });
            }

            if let Some(origin) = manifest
                .origin
                .as_deref()
                .and_then(|origin| Url::parse(origin).ok())
            {
                return Ok(PackageSpecifier::Url(origin));
            }

            Err(Error::msg(
                "Unable to determine a package specifier for a vendored dependency",
            ))
        }
        UrlOrManifest::RegistryDependentUrl(specifier) => specifier.parse(),
    }
}

fn file_hash(path: &Path) -> Result<[u8; 32], Error> {
    let mut hasher = Sha256::default();
    let mut reader = BufReader::new(File::open(path)?);

    loop {
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            break;
        }
        hasher.update(buffer);
        let bytes_read = buffer.len();
        reader.consume(bytes_read);
    }

    Ok(hasher.finalize().into())
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

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
            Summary {
                package_name: "sharrattj/bash".to_string(),
                version: "1.0.12".parse().unwrap(),
                webc: Url::from_file_path(bash.canonicalize().unwrap()).unwrap(),
                webc_sha256: [
                    7, 226, 190, 131, 173, 231, 130, 245, 207, 185, 51, 189, 86, 85, 222, 37, 27,
                    163, 170, 27, 25, 24, 211, 136, 186, 233, 174, 119, 66, 15, 134, 9
                ],
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
                source: source.id()
            }
        );
    }
}
