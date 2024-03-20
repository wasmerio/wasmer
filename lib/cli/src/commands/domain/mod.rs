pub mod get;
pub mod list;
pub mod register;
pub mod zonefile;
use crate::commands::AsyncCliCommand;

/// Manage DNS records
#[derive(clap::Subcommand, Debug)]
pub enum CmdDomain {
    /// List domains
    List(self::list::CmdDomainList),

    /// Get a domain
    Get(self::get::CmdDomainGet),

    /// Get zone file for a domain
    GetZoneFile(self::zonefile::CmdZoneFileGet),

    /// Sync local zone file with remotex
    SyncZoneFile(self::zonefile::CmdZoneFileSync),

    /// Register new domain
    Register(self::register::CmdDomainRegister),
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdDomain {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        match self {
            CmdDomain::List(cmd) => cmd.run_async().await,
            CmdDomain::Get(cmd) => cmd.run_async().await,
            CmdDomain::GetZoneFile(cmd) => cmd.run_async().await,
            CmdDomain::SyncZoneFile(cmd) => cmd.run_async().await,
            CmdDomain::Register(cmd) => cmd.run_async().await,
        }
    }
}
