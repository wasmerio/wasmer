use std::path::PathBuf;

use anyhow::Context;
use dialoguer::console::{Emoji, style};
use indicatif::ProgressBar;
use sha2::Digest;
use wasmer_config::package::{NamedPackageId, PackageHash, Webcm};
use wasmer_package::package::Package;

use crate::utils::load_package_manifest;

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

    /// Don't write the `.webcm` sidecar manifest next to the built package.
    ///
    /// The sidecar records the package's name, version, and hash, which the
    /// `.webc` itself does not retain. Local tooling (e.g. `wasmer run
    /// --include-webc`) uses it to identify the package.
    #[clap(long)]
    no_webcm: bool,
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
            no_webcm: true,
        }
    }

    pub(crate) fn execute(&self) -> Result<(Package, PackageHash), anyhow::Error> {
        let manifest_path = self.manifest_path()?;
        let Some((_, manifest)) = load_package_manifest(&manifest_path)? else {
            anyhow::bail!(
                "Could not locate manifest in path '{}'",
                manifest_path.display()
            )
        };
        let pkg = Package::from_manifest(manifest_path.clone()).context(format!(
            "While parsing the manifest (loaded from {})",
            manifest_path.canonicalize()?.display()
        ))?;
        let data = pkg.serialize().context("While validating the package")?;
        let hash = sha2::Sha256::digest(&data).into();
        let pkg_hash = PackageHash::from_sha256_bytes(hash);

        let manifest_pkg = manifest.package.as_ref();
        let ident = manifest_pkg.and_then(|p| {
            Some(NamedPackageId {
                full_name: p.name.clone()?,
                version: p.version.clone()?,
            })
        });

        let name = if let Some(ident) = &ident {
            ident.webc_file_name()
        } else if let Some(name) = manifest_pkg.and_then(|p| p.name.as_deref()) {
            format!("{}-{}.webc", name.replace('/', "-"), pkg_hash)
        } else {
            format!("{pkg_hash}.webc")
        };

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

        // rest of the code writes the package to disk and is irrelevant
        // to checking.
        if self.check {
            return Ok((pkg, pkg_hash));
        }

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

        pb.println(format!(
            "{} {}Writing package...",
            style("[3/3]").bold().dim(),
            WRITING_PACKAGE_EMOJI
        ));

        std::fs::write(&out_path, &data)
            .with_context(|| format!("could not write contents to '{}'", out_path.display()))?;

        // Only named packages have an identity worth recording in a sidecar.
        if !self.no_webcm
            && let Some(ident) = ident
        {
            let webcm_path = Webcm::path_for_webc(&out_path);
            let webcm = Webcm::new(ident, Some(pkg_hash.clone()));
            std::fs::write(&webcm_path, webcm.to_toml()?)
                .with_context(|| format!("could not write '{}'", webcm_path.display()))?;

            pb.println(format!(
                "{WRITING_PACKAGE_EMOJI}Sidecar manifest written to '{}'",
                webcm_path.display()
            ));
        }

        pb.finish_with_message(format!(
            "{} Package written to '{}'",
            SPARKLE,
            out_path.display()
        ));

        Ok((pkg, pkg_hash))
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
    use wasmer_package::utils::from_disk;

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
            no_webcm: false,
        };

        let (_, pkg_hash) = cmd.execute().unwrap();

        from_disk(path.join("wasmer-hello-0.1.0.webc")).unwrap();

        let webcm: Webcm = std::fs::read_to_string(path.join("wasmer-hello-0.1.0.webcm"))
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(
            webcm.id(),
            NamedPackageId::try_new("wasmer/hello", "0.1.0").unwrap()
        );
        assert_eq!(webcm.package.hash, Some(pkg_hash));
    }
}
