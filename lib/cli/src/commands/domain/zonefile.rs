use crate::commands::AsyncCliCommand;

#[derive(clap::Parser, Debug)]
/// Show a zone file
pub struct CmdZoneFileGet {
    /// Name of the domain.
        domain_name: String,

    /// output file name to store zone file
        #[clap(short='o', long="output", required = false)]
        zone_file_path: Option<String>,
}

#[derive(clap::Parser, Debug)]
/// Show a zone file
pub struct CmdZoneFileSync {
    /// filename of  zone-file to sync
        zone_file_path: String,
}


#[async_trait::async_trait]
impl AsyncCliCommand for CmdZoneFileGet {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        Ok(())
    }
}



#[async_trait::async_trait]
impl AsyncCliCommand for CmdZoneFileSync {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        Ok(())
    }
}
