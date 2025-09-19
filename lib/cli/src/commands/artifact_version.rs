use clap::Parser;
use wasmer_types::MetadataHeader;

use crate::commands::CliCommand;

#[derive(Debug, Parser)]
pub struct CmdArtifactVersion {}

impl CliCommand for CmdArtifactVersion {
    type Output = ();

    fn run(self) -> Result<Self::Output, anyhow::Error> {
        println!("{}", MetadataHeader::CURRENT_VERSION);
        Ok(())
    }
}
