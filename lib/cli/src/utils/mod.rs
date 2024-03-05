//! Utility functions for the WebAssembly module

pub(crate) mod package_wizard;
pub(crate) mod prompts;
pub(crate) mod render;

use std::{
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::{bail, Context as _, Result};
use edge_schema::schema::StringWebcIdent;
use once_cell::sync::Lazy;
use regex::Regex;
use wasmer_api::WasmerClient;
use wasmer_wasix::runners::MappedDirectory;

fn retrieve_alias_pathbuf(alias: &str, real_dir: &str) -> Result<MappedDirectory> {
    let pb = PathBuf::from(&real_dir).canonicalize()?;
    if let Ok(pb_metadata) = pb.metadata() {
        if !pb_metadata.is_dir() {
            bail!("\"{}\" exists, but it is not a directory", &real_dir);
        }
    } else {
        bail!("Directory \"{}\" does not exist", &real_dir);
    }
    Ok(MappedDirectory {
        guest: alias.to_string(),
        host: pb,
    })
}

/// Parses a mapdir from a string
pub fn parse_mapdir(entry: &str) -> Result<MappedDirectory> {
    // We try first splitting by `::`
    if let [alias, real_dir] = entry.split("::").collect::<Vec<&str>>()[..] {
        retrieve_alias_pathbuf(alias, real_dir)
    }
    // And then we try splitting by `:` (for compatibility with previous API)
    else if let [alias, real_dir] = entry.splitn(2, ':').collect::<Vec<&str>>()[..] {
        retrieve_alias_pathbuf(alias, real_dir)
    } else {
        bail!(
            "Directory mappings must consist of two paths separate by a `::` or `:`. Found {}",
            &entry
        )
    }
}

/// Parses an environment variable.
pub fn parse_envvar(entry: &str) -> Result<(String, String)> {
    let entry = entry.trim();

    match entry.find('=') {
        None => bail!(
            "Environment variable must be of the form `<name>=<value>`; found `{}`",
            &entry
        ),

        Some(0) => bail!(
            "Environment variable is not well formed, the `name` is missing in `<name>=<value>`; got `{}`",
            &entry
        ),

        Some(position) if position == entry.len() - 1 => bail!(
            "Environment variable is not well formed, the `value` is missing in `<name>=<value>`; got `{}`",
            &entry
        ),

        Some(position) => Ok((entry[..position].into(), entry[position + 1..].into())),
    }
}

pub(crate) const DEFAULT_PACKAGE_MANIFEST_FILE: &str = "wasmer.toml";

/// Load a package manifest from the manifest file.
///
/// Path can either be a directory, or a concrete file path.
pub fn load_package_manifest(
    path: &Path,
) -> Result<Option<(PathBuf, wasmer_toml::Manifest)>, anyhow::Error> {
    let file_path = if path.is_file() {
        path.to_owned()
    } else {
        path.join(DEFAULT_PACKAGE_MANIFEST_FILE)
    };

    let contents = match std::fs::read_to_string(&file_path) {
        Ok(c) => c,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(err).with_context(|| {
                format!(
                    "Could not read package manifest at '{}'",
                    file_path.display()
                )
            })
        }
    };
    let manifest = wasmer_toml::Manifest::parse(&contents).with_context(|| {
        format!(
            "Could not parse package config at: '{}'",
            file_path.display()
        )
    })?;
    Ok(Some((file_path, manifest)))
}

/// Ask a user for a package name.
///
/// Will continue looping until the user provides a valid name.
pub fn prompt_for_package_name(
    message: &str,
    default: Option<&str>,
) -> Result<StringWebcIdent, anyhow::Error> {
    loop {
        let raw: String = dialoguer::Input::new()
            .with_prompt(message)
            .with_initial_text(default.unwrap_or_default())
            .interact_text()
            .context("could not read user input")?;

        match raw.parse::<StringWebcIdent>() {
            Ok(p) => break Ok(p),
            Err(err) => {
                eprintln!("invalid package name: {err}");
            }
        }
    }
}

/// Defines how to check for a package.
pub enum PackageCheckMode {
    /// The package must exist in the registry.
    MustExist,
    /// The package must NOT exist in the registry.
    #[allow(dead_code)]
    MustNotExist,
}

/// Ask for a package name.
///
/// Will continue looping until the user provides a valid name.
///
/// If an API is provided, will check if the package exists.
pub async fn prompt_for_package(
    message: &str,
    default: Option<&str>,
    check: Option<PackageCheckMode>,
    client: Option<&WasmerClient>,
) -> Result<(StringWebcIdent, Option<wasmer_api::types::Package>), anyhow::Error> {
    loop {
        let name = prompt_for_package_name(message, default)?;

        if let Some(check) = &check {
            let api = client.expect("Check mode specified, but no API provided");

            let pkg = wasmer_api::query::get_package(api, name.to_string())
                .await
                .context("could not query backend for package")?;

            match check {
                PackageCheckMode::MustExist => {
                    if let Some(pkg) = pkg {
                        break Ok((name, Some(pkg)));
                    } else {
                        eprintln!("Package '{name}' does not exist");
                    }
                }
                PackageCheckMode::MustNotExist => {
                    if pkg.is_none() {
                        break Ok((name, None));
                    } else {
                        eprintln!("Package '{name}' already exists");
                    }
                }
            }
        }
    }
}

/// Re-publish a package with an increased minor version.
pub async fn republish_package_with_bumped_version(
    client: &WasmerClient,
    manifest_path: &Path,
    mut manifest: wasmer_toml::Manifest,
) -> Result<wasmer_toml::Manifest, anyhow::Error> {
    // Try to load existing version.
    // If it does not exist yet, we don't need to increment.

    let current_opt = wasmer_api::query::get_package(client, manifest.package.name.clone())
        .await
        .context("could not load package info from backend")?
        .and_then(|x| x.last_version);

    let new_version = if let Some(current) = &current_opt {
        let mut v = semver::Version::parse(&current.version)
            .with_context(|| format!("Could not parse package version: '{}'", current.version))?;

        v.patch += 1;

        // The backend does not have a reliable way to return the latest version,
        // so we have to check each version in a loop.
        loop {
            let version = format!("={}", v);
            let version = wasmer_api::query::get_package_version(
                client,
                manifest.package.name.clone(),
                version.clone(),
            )
            .await
            .context("could not load package info from backend")?;

            if version.is_some() {
                v.patch += 1;
            } else {
                break;
            }
        }

        v
    } else {
        manifest.package.version
    };

    manifest.package.version = new_version;
    let contents = toml::to_string(&manifest).with_context(|| {
        format!(
            "could not persist manifest to '{}'",
            manifest_path.display()
        )
    })?;

    let manifest_path = if manifest_path.is_file() {
        manifest_path.to_owned()
    } else {
        manifest_path.join(DEFAULT_PACKAGE_MANIFEST_FILE)
    };

    std::fs::write(manifest_path.clone(), contents)
        .with_context(|| format!("could not write manifest to '{}'", manifest_path.display()))?;

    let dir = manifest_path
        .parent()
        .context("could not determine wasmer.toml parent directory")?
        .to_owned();

    let registry = client.graphql_endpoint().to_string();
    let token = client
        .auth_token()
        .context("no auth token configured - run 'wasmer login'")?
        .to_string();

    let publish = wasmer_registry::package::builder::Publish {
        registry: Some(registry),
        dry_run: false,
        quiet: false,
        package_name: None,
        version: None,
        wait: wasmer_registry::publish::PublishWait::new_none(),
        token,
        no_validate: true,
        package_path: Some(dir.to_str().unwrap().to_string()),
        // Use a high timeout to prevent interrupting uploads of
        // large packages.
        timeout: std::time::Duration::from_secs(60 * 60 * 12),
    };

    // Publish uses a blocking http client internally, which leads to a
    // "can't drop a runtime within an async context" error, so this has
    // to be run in a separate thread.
    std::thread::spawn(move || publish.execute())
        .join()
        .map_err(|e| anyhow::format_err!("failed to publish package: {:?}", e))??;

    Ok(manifest)
}

/// The identifier for an app or package in the form, `owner/package@version`,
/// where the `owner` and `version` are optional.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Identifier {
    /// The package's name.
    pub name: String,
    /// The package's owner, typically a username or namespace.
    pub owner: Option<String>,
    /// The package's version number.
    pub version: Option<String>,
}

impl Identifier {
    pub fn new(name: impl Into<String>) -> Self {
        Identifier {
            name: name.into(),
            owner: None,
            version: None,
        }
    }

    pub fn with_owner(self, owner: impl Into<String>) -> Self {
        Identifier {
            owner: Some(owner.into()),
            ..self
        }
    }

    pub fn with_version(self, version: impl Into<String>) -> Self {
        Identifier {
            version: Some(version.into()),
            ..self
        }
    }
}

impl FromStr for Identifier {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        const PATTERN: &str = r"^(?x)
            (?:
                (?P<owner>[a-zA-Z][\w\d_.-]*)
                /
            )?
            (?P<name>[a-zA-Z][\w\d_.-]*)
            (?:
                @
                (?P<version>[\w\d.]+)
            )?
            $
        ";
        static RE: Lazy<Regex> = Lazy::new(|| Regex::new(PATTERN).unwrap());

        let caps = RE.captures(s).context(
            "Invalid package identifier, expected something like namespace/package@version",
        )?;

        let mut identifier = Identifier::new(&caps["name"]);

        if let Some(owner) = caps.name("owner") {
            identifier = identifier.with_owner(owner.as_str());
        }

        if let Some(version) = caps.name("version") {
            identifier = identifier.with_version(version.as_str());
        }

        Ok(identifier)
    }
}

/// Merge two yaml values by recursively merging maps from b into a.
///
/// Preserves old values that were not in b.
pub(crate) fn merge_yaml_values(a: &serde_yaml::Value, b: &serde_yaml::Value) -> serde_yaml::Value {
    use serde_yaml::Value as V;
    match (a, b) {
        (V::Mapping(a), V::Mapping(b)) => {
            let mut m = a.clone();
            for (k, v) in b.iter() {
                let newval = if let Some(old) = a.get(k) {
                    merge_yaml_values(old, v)
                } else {
                    v.clone()
                };
                m.insert(k.clone(), newval);
            }
            V::Mapping(m)
        }
        _ => b.clone(),
    }
}

// pub(crate) fn shell(cmd: &str, args: &[&str]) -> anyhow::Result<Option<i32>> {
//     let ecode = Command::new(cmd).args(args).spawn()?.wait()?;
//     if ecode.success() {
//         Ok(ecode.code())
//     } else {
//         Err(anyhow::format_err!(
//             "failed to execute linux command (code={:?})",
//             ecode.code()
//         ))
//     }
// }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_yaml_values() {
        use serde_yaml::Value;
        let v1 = r#"
a: a
b:
  b1: b1
c: c
        "#;
        let v2 = r#"
a: a1
b:
  b2: b2
        "#;
        let v3 = r#"
a: a1
b:
  b1: b1
  b2: b2
c: c
        "#;

        let a: Value = serde_yaml::from_str(v1).unwrap();
        let b: Value = serde_yaml::from_str(v2).unwrap();
        let c: Value = serde_yaml::from_str(v3).unwrap();
        let merged = merge_yaml_values(&a, &b);
        assert_eq!(merged, c);
    }

    #[test]
    fn parse_valid_identifiers() {
        let inputs = [
            ("python", Identifier::new("python")),
            (
                "syrusakbary/python",
                Identifier::new("python").with_owner("syrusakbary"),
            ),
            (
                "wasmer/wasmer.io",
                Identifier::new("wasmer.io").with_owner("wasmer"),
            ),
            (
                "syrusakbary/python@1.2.3",
                Identifier::new("python")
                    .with_owner("syrusakbary")
                    .with_version("1.2.3"),
            ),
            (
                "python@1.2.3",
                Identifier::new("python").with_version("1.2.3"),
            ),
        ];

        for (src, expected) in inputs {
            let identifier = Identifier::from_str(src).expect(src);
            assert_eq!(identifier, expected);
        }
    }

    #[test]
    fn invalid_package_identifiers() {
        let inputs = ["", "$", "python/", "/python", "python@", "."];

        for input in inputs {
            let result = Identifier::from_str(input);
            assert!(result.is_err(), "Got {result:?} from {input:?}");
        }
    }

    #[test]
    fn test_parse_envvar() {
        assert_eq!(
            parse_envvar("A").unwrap_err().to_string(),
            "Environment variable must be of the form `<name>=<value>`; found `A`"
        );
        assert_eq!(
            parse_envvar("=A").unwrap_err().to_string(),
            "Environment variable is not well formed, the `name` is missing in `<name>=<value>`; got `=A`"
        );
        assert_eq!(
            parse_envvar("A=").unwrap_err().to_string(),
            "Environment variable is not well formed, the `value` is missing in `<name>=<value>`; got `A=`"
        );
        assert_eq!(parse_envvar("A=B").unwrap(), ("A".into(), "B".into()));
        assert_eq!(parse_envvar("   A=B\t").unwrap(), ("A".into(), "B".into()));
        assert_eq!(
            parse_envvar("A=B=C=D").unwrap(),
            ("A".into(), "B=C=D".into())
        );
    }
}
