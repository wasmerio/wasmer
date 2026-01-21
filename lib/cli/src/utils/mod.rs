//! Utility functions for the WebAssembly module

pub(crate) mod package_wizard;
pub(crate) mod prompts;
pub(crate) mod render;
pub(crate) mod timestamp;
pub(crate) mod unpack;

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use itertools::Itertools;
use wasmer_wasix::runners::MappedDirectory;

fn retrieve_alias_pathbuf(host_dir: &str, guest_dir: &str) -> Result<MappedDirectory> {
    let host_dir_path = PathBuf::from(&host_dir).canonicalize()?;
    if let Ok(pb_metadata) = host_dir_path.metadata() {
        if !pb_metadata.is_dir() {
            bail!("\"{}\" exists, but it is not a directory", &host_dir);
        }
    } else {
        bail!("Directory \"{}\" does not exist", &host_dir);
    }
    Ok(MappedDirectory {
        host: host_dir_path,
        guest: guest_dir.to_string(),
    })
}

/// Parses a volume from a string
pub fn parse_volume(entry: &str) -> Result<MappedDirectory> {
    let components = entry.split(":").collect_vec();
    match components.as_slice() {
        [entry] => retrieve_alias_pathbuf(entry, entry),
        [host_dir, guest_dir] => retrieve_alias_pathbuf(host_dir, guest_dir),
        _ => bail!(
            "Directory mappings must consist of a single path, or two paths separate by a `:`. Found {}",
            &entry
        ),
    }
}

/// Parses a mapdir(legacy option) from a string.
pub fn parse_mapdir(entry: &str) -> Result<MappedDirectory> {
    let components = entry.split(":").collect_vec();
    match components.as_slice() {
        [entry] => retrieve_alias_pathbuf(entry, entry),
        // swapper order compared to the --volume option
        [guest_dir, host_dir] => retrieve_alias_pathbuf(host_dir, guest_dir),
        _ => bail!(
            "Directory mappings must consist of a single path, or two paths separate by a `:`. Found {}",
            &entry
        ),
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
) -> Result<Option<(PathBuf, wasmer_config::package::Manifest)>, anyhow::Error> {
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
            });
        }
    };

    let manifest = wasmer_config::package::Manifest::parse(&contents).with_context(|| {
        format!(
            "Could not parse package config at: '{}' - full config: {}",
            file_path.display(),
            contents
        )
    })?;

    Ok(Some((file_path, manifest)))
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
