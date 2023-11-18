mod unpack;

pub use unpack::PackageUnpack;

/// Container related commands. (inspecting, unpacking, ...)
#[derive(clap::Subcommand, Debug)]
// Allowing missing_docs because the comment would override the doc comment on
// the command struct.
#[allow(missing_docs)]
pub enum Container {
    Unpack(PackageUnpack),
}
