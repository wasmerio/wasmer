mod build;
mod download;

pub use build::PackageBuild;
pub use download::PackageDownload;

/// Package related commands.
#[derive(clap::Subcommand, Debug)]
// Allowing missing_docs because the comment would override the doc comment on
// the command struct.
#[allow(missing_docs)]
pub enum Package {
    Download(PackageDownload),
    Build(build::PackageBuild),
}
