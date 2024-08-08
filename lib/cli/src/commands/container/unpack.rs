/// RENAMED: the 'container unpack' command has been renamed to 'package unpack'!
#[derive(clap::Parser, Debug)]
pub struct PackageUnpack {}

impl PackageUnpack {
    pub(crate) fn execute(&self) -> Result<(), anyhow::Error> {
        anyhow::bail!("This command was renamed: use 'wasmer package unpack instead'");
    }
}
