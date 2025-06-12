use std::path::PathBuf;

use anyhow::Context;
use crate::{commands::AsyncCliCommand, config::WasmerEnv, opts::ItemFormatOpts};

/// Create a new vault.
#[derive(clap::Parser, Debug)]
pub struct CmdVaultCreate {
    #[clap(flatten)]
    fmt: ItemFormatOpts,

    #[clap(flatten)]
    env: WasmerEnv,

    /// File containing vault schema
    #[clap(value_name = "FILE", default_value = "vault.yaml")]
    filename: PathBuf,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdVaultCreate {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        
        if !self.filename.try_exists().context("checking filename existance")? {
            anyhow::bail!("Vault file does not exist");
        }
        if !self.filename.is_file() {
            anyhow::bail!("File at path specified by `filename` argument is not valid. Maybe it's a directory?");
        }

        let data = std::fs::read(&self.filename).context("Unable to read file")?;
        let vault_contents = String::from_utf8(data).context("Not a valid UTF-8 sequence")?;
        let vault = wasmer_backend_api::query::upsert_vault(
            &self.env.client()?,
            &vault_contents
        ).await.context("Upserting vault")?;

        println!("vault created: {}", vault.name);

        let mut yaml_value: serde_yaml::Value = serde_yaml::from_str(&vault_contents)
            .context("Parsing vault file into YAML")?;


        if let serde_yaml::Value::Mapping(ref mut map) = yaml_value {
            map.insert(
                serde_yaml::Value::String("vault_id".to_string()),
                serde_yaml::Value::String(vault.id.into_inner()),
            );
        } else {
            anyhow::bail!("Vault file does not have a valid YAML object at root");
        }

        let updated_yaml = serde_yaml::to_string(&yaml_value).context("Serializing updated vault file")?;

        std::fs::write(&self.filename, updated_yaml)
            .context("Writing updated vault file with vault_id")?;

        println!("vault_id written back into {}", self.filename.display());

        Ok(())
    }
}
