mod build;
mod common;
mod download;
pub mod publish;
mod push;
mod tag;
mod unpack;

pub use build::PackageBuild;
pub use common::wait::PublishWait;

/// Package related commands.
#[derive(clap::Subcommand, Debug)]
// Allowing missing_docs because the comment would override the doc comment on
// the command struct.
#[allow(missing_docs)]
#[allow(clippy::large_enum_variant)]
pub enum Package {
    Download(download::PackageDownload),
    Build(build::PackageBuild),
    Tag(tag::PackageTag),
    Push(push::PackagePush),
    Publish(publish::PackagePublish),
    Unpack(unpack::PackageUnpack),
}
