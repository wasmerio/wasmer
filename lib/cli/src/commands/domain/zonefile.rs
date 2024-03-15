use crate::{
    commands::AsyncCliCommand,
    opts::{ApiOpts, ItemFormatOpts},
};
use anyhow::Context;

#[derive(clap::Parser, Debug)]
/// Show a zone file
pub struct CmdZoneFileGet {
    #[clap(flatten)]
    fmt: ItemFormatOpts,

    #[clap(flatten)]
    api: ApiOpts,

    /// Name of the domain.
    domain_name: String,

    /// output file name to store zone file
    #[clap(short = 'o', long = "output", required = false)]
    zone_file_path: Option<String>,
}

#[derive(clap::Parser, Debug)]
/// Show a zone file
pub struct CmdZoneFileSync {
    #[clap(flatten)]
    api: ApiOpts,

    /// filename of  zone-file to sync
    zone_file_path: String,

    /// Do not delete records that are not present in the zone file
    #[clap(short = 'n', long = "no-delete-missing-records")]
    no_delete_missing_records: bool,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdZoneFileGet {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        let client = self.api.client()?;
        if let Some(domain) =
            wasmer_api::query::get_domain_zone_file(&client, self.domain_name).await?
        {
            let zone_file_contents = domain.zone_file;
            if let Some(zone_file_path) = self.zone_file_path {
                std::fs::write(zone_file_path, zone_file_contents)
                    .context("Unable to write file")?;
            } else {
                println!("{}", zone_file_contents);
            }
        } else {
            anyhow::bail!("Domain not found");
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdZoneFileSync {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        let data = std::fs::read(&self.zone_file_path).context("Unable to read file")?;
        let zone_file_contents = String::from_utf8(data).context("Not a valid UTF-8 sequence")?;
        let domain = wasmer_api::query::upsert_domain_from_zone_file(
            &self.api.client()?,
            zone_file_contents,
            !self.no_delete_missing_records,
        )
        .await?;
        println!("Successfully synced domain: {}", domain.name);
        Ok(())
    }
}
