use std::path::PathBuf;

use anyhow::Context;
use dialoguer::console::{style, Emoji};
use indicatif::ProgressBar;
use wasmer_package::utils::from_disk;

/// Extract contents of a webc image to a directory.
///
/// See --format flag for available output formats.
#[derive(clap::Parser, Debug)]
pub struct PackageUnpack {
    /// The output directory.
    #[clap(short = 'o', long)]
    pub out_dir: PathBuf,

    /// Overwrite existing directories/files.
    #[clap(long)]
    pub overwrite: bool,

    /// Run the unpack command without any output
    #[clap(long)]
    pub quiet: bool,

    /// Path to the package.
    pub package_path: PathBuf,

    /// Output format.
    ///
    /// * package
    ///   Restore a package directory with a wasmer.toml
    ///   NOTE: this conversion is lossy, because webcs don't store the original
    ///   wasmer.toml and the full contents can not be restored.
    ///
    /// * webc
    ///   Directly unpack the webc contents.
    ///   - Volumes will be placed in subdirectories.
    ///   - atoms will be placed in the root directory
    ///   - the full webc manifest will be placed in a manifest.json file
    #[clap(short, long, default_value = "package")]
    pub format: Format,
}

static PACKAGE_EMOJI: Emoji<'_, '_> = Emoji("📦 ", "");
static EXTRACTED_TO_EMOJI: Emoji<'_, '_> = Emoji("📂 ", "");

/// Webc unpack format.
#[derive(clap::ValueEnum, Clone, Debug)]
pub enum Format {
    /// See [`PackageUnpack::format`] for details.
    Package,
    /// See [`PackageUnpack::format`] for details.
    Webc,
}

impl PackageUnpack {
    pub(crate) fn execute(&self) -> Result<(), anyhow::Error> {
        // Setup the progress bar
        let pb = if self.quiet {
            ProgressBar::hidden()
        } else {
            ProgressBar::new_spinner()
        };

        pb.println(format!(
            "{} {}Unpacking...",
            style("[1/2]").bold().dim(),
            PACKAGE_EMOJI
        ));

        let pkg = from_disk(&self.package_path).with_context(|| {
            format!(
                "could not open package at '{}'",
                self.package_path.display()
            )
        })?;

        let outdir = &self.out_dir;
        std::fs::create_dir_all(outdir)
            .with_context(|| format!("could not create output directory '{}'", outdir.display()))?;

        match self.format {
            Format::Package => {
                wasmer_package::convert::webc_to_package_dir(&pkg, outdir)
                    .with_context(|| "could not extract package")?;
            }
            Format::Webc => {
                pkg.unpack(outdir, self.overwrite)
                    .with_context(|| "could not extract package".to_string())?;
            }
        }

        pb.println(format!(
            "{} {}Extracted package contents to '{}'",
            style("[2/2]").bold().dim(),
            EXTRACTED_TO_EMOJI,
            self.out_dir.display()
        ));

        pb.finish();

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Download a package from the dev registry.
    #[test]
    fn test_cmd_package_extract() {
        let dir = tempfile::tempdir().unwrap();

        let package_path = std::env::var("CARGO_MANIFEST_DIR").map(PathBuf::from).unwrap()
            .parent().unwrap()
            .parent().unwrap()
            .join("tests/integration/cli/tests/webc/hello-0.1.0-665d2ddc-80e6-4845-85d3-4587b1693bb7.webc");

        assert!(package_path.is_file());

        let cmd = PackageUnpack {
            out_dir: dir.path().to_owned(),
            overwrite: false,
            package_path,
            quiet: true,
            format: Format::Webc,
        };

        cmd.execute().unwrap();

        let mut items = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|x| {
                x.unwrap()
                    .path()
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_string()
            })
            .collect::<Vec<_>>();
        items.sort();
        assert_eq!(
            items,
            vec![
                "atom".to_string(),
                "manifest.json".to_string(),
                "metadata".to_string(),
            ]
        );
    }
}
