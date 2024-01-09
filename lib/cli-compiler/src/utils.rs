//! Utility functions for the WebAssembly module
use anyhow::{bail, Context, Result};
use is_terminal::IsTerminal;
use std::path::PathBuf;
use std::{env, path::Path};

/// Whether or not Wasmer should print with color
pub fn wasmer_should_print_color() -> bool {
    env::var("WASMER_COLOR")
        .ok()
        .and_then(|inner| inner.parse::<bool>().ok())
        .unwrap_or_else(|| std::io::stdout().is_terminal())
}

fn retrieve_alias_pathbuf(alias: &str, real_dir: &str) -> Result<(String, PathBuf)> {
    let pb = Path::new(real_dir)
        .canonicalize()
        .with_context(|| format!("Unable to get the absolute path for \"{real_dir}\""))?;

    if let Ok(pb_metadata) = pb.metadata() {
        if !pb_metadata.is_dir() {
            bail!("\"{real_dir}\" exists, but it is not a directory");
        }
    } else {
        bail!("Directory \"{real_dir}\" does not exist");
    }

    Ok((alias.to_string(), pb))
}

/// Parses a mapdir from a string
pub fn parse_mapdir(entry: &str) -> Result<(String, PathBuf)> {
    // We try first splitting by `::`
    if let [alias, real_dir] = entry.split("::").collect::<Vec<&str>>()[..] {
        retrieve_alias_pathbuf(alias, real_dir)
    }
    // And then we try splitting by `:` (for compatibility with previous API)
    else if let [alias, real_dir] = entry.split(':').collect::<Vec<&str>>()[..] {
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

#[cfg(test)]
mod tests {
    use super::parse_envvar;

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
