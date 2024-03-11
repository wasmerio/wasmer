use std::path::PathBuf;

use anyhow::Context;
use dialoguer::console::{style, Emoji};
use indicatif::ProgressBar;

/// Build a container from a package manifest.
#[derive(clap::Parser, Debug)]
pub struct PackageBuild {
    /// Output path for the package file.
    /// Defaults to current directory + [name]-[version].webc.
    #[clap(short = 'o', long)]
    out: Option<PathBuf>,

    /// Run the publish command without any output
    #[clap(long)]
    pub quiet: bool,

    /// Path of the package or wasmer.toml manifest.
    ///
    /// Defaults to current directory.
    package: Option<PathBuf>,

    /// Only checks whether the package could be built successfully
    #[clap(long)]
    check: bool,
}

static READING_MANIFEST_EMOJI: Emoji<'_, '_> = Emoji("📖 ", "");
static CREATING_OUTPUT_DIRECTORY_EMOJI: Emoji<'_, '_> = Emoji("📁 ", "");
static WRITING_PACKAGE_EMOJI: Emoji<'_, '_> = Emoji("📦 ", "");
static SPARKLE: Emoji<'_, '_> = Emoji("✨ ", ":-)");

impl PackageBuild {
    pub(crate) fn check(package_path: PathBuf) -> Self {
        PackageBuild {
            out: None,
            quiet: true,
            package: Some(package_path),
            check: true,
        }
    }

    pub(crate) fn execute(&self) -> Result<(), anyhow::Error> {
        let manifest_path = self.manifest_path()?;
        let pkg = webc::wasmer_package::Package::from_manifest(manifest_path)?;

        // Setup the progress bar
        let pb = if self.quiet {
            ProgressBar::hidden()
        } else {
            ProgressBar::new_spinner()
        };

        pb.println(format!(
            "{} {}Reading manifest...",
            style("[1/3]").bold().dim(),
            READING_MANIFEST_EMOJI
        ));

        let manifest = pkg
            .manifest()
            .wapm()
            .context("could not load package manifest")?
            .context("package does not contain a Wasmer manifest")?;

        // rest of the code writes the package to disk and is irrelevant
        // to checking.
        if self.check {
            return Ok(());
        }

        let pkgname = manifest.name.replace('/', "-");
        let name = format!("{}-{}.webc", pkgname, manifest.version,);

        pb.println(format!(
            "{} {}Creating output directory...",
            style("[2/3]").bold().dim(),
            CREATING_OUTPUT_DIRECTORY_EMOJI
        ));

        let out_path = if let Some(p) = &self.out {
            if p.is_dir() {
                p.join(name)
            } else {
                if let Some(parent) = p.parent() {
                    std::fs::create_dir_all(parent).context("could not create output directory")?;
                }

                p.to_owned()
            }
        } else {
            std::env::current_dir()
                .context("could not determine current directory")?
                .join(name)
        };

        if out_path.exists() {
            anyhow::bail!(
                "Output path '{}' already exists - specify a different path with -o/--out",
                out_path.display()
            );
        }

        let data = pkg.serialize()?;

        pb.println(format!(
            "{} {}Writing package...",
            style("[3/3]").bold().dim(),
            WRITING_PACKAGE_EMOJI
        ));

        std::fs::write(&out_path, &data)
            .with_context(|| format!("could not write contents to '{}'", out_path.display()))?;

        pb.finish_with_message(format!(
            "{} Package written to '{}'",
            SPARKLE,
            out_path.display()
        ));

        Ok(())
    }

    fn manifest_path(&self) -> Result<PathBuf, anyhow::Error> {
        let path = if let Some(p) = &self.package {
            if p.is_dir() {
                let manifest_path = p.join("wasmer.toml");
                if !manifest_path.is_file() {
                    anyhow::bail!(
                        "Specified directory '{}' does not contain a wasmer.toml manifest",
                        p.display()
                    );
                }
                manifest_path
            } else if p.is_file() {
                p.clone()
            } else {
                anyhow::bail!(
                    "Specified path '{}' is not a file or directory",
                    p.display()
                );
            }
        } else {
            let dir = std::env::current_dir().context("could not get current directory")?;
            let manifest_path = dir.join("wasmer.toml");
            if !manifest_path.is_file() {
                anyhow::bail!(
                    "Current directory '{}' does not contain a wasmer.toml manifest - specify a path with --package-dir",
                    dir.display()
                );
            }
            manifest_path
        };

        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Download a package from the dev registry.
    #[test]
    fn test_cmd_package_build() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();

        std::fs::write(
            path.join("wasmer.toml"),
            r#"
[package]
name = "wasmer/hello"
version = "0.1.0"
description = "hello"

[fs]
"data" = "data"
"#,
        )
        .unwrap();

        std::fs::create_dir(path.join("data")).unwrap();
        std::fs::write(path.join("data").join("hello.txt"), "Hello, world!").unwrap();

        let cmd = PackageBuild {
            package: Some(path.to_owned()),
            out: Some(path.to_owned()),
            quiet: true,
            check: false,
        };

        cmd.execute().unwrap();

        webc::Container::from_disk(path.join("wasmer-hello-0.1.0.webc")).unwrap();
    }
}
