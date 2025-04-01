//! Edge SSH command.

use anyhow::Context;
use wasmer_backend_api::WasmerClient;

use super::AsyncCliCommand;
use crate::{config::WasmerEnv, edge_config::EdgeConfig};

/// Start a remote SSH session.
#[derive(clap::Parser, Debug)]
pub struct CmdSsh {
    #[clap(flatten)]
    env: WasmerEnv,
    /// SSH port to use.
    #[clap(long, default_value = "22")]
    pub ssh_port: u16,
    /// Local port mapping to the package that's running, this allows
    /// for instance a HTTP server to be tested remotely while giving
    /// instant logs over stderr channelled via SSH.
    #[clap(long)]
    pub map_port: Vec<u16>,
    /// Package to run on the Deploy servers
    ///
    /// By default a bash session will be started.
    #[clap(index = 1)]
    pub run: Option<String>,

    /// Additional arguments to pass to the package.
    #[clap(index = 2, trailing_var_arg = true)]
    pub run_args: Vec<String>,

    /// The Edge SSH server host to connect to.
    ///
    /// You can usually ignore this flag, it mainly exists for debugging purposes.
    #[clap(long)]
    pub host: Option<String>,

    /// Prints the SSH command rather than executing it.
    #[clap(short, long)]
    pub print: bool,
}

impl CmdSsh {
    /// The default package to run.
    const DEFAULT_PACKAGE: &'static str = "wasmer/bash";
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdSsh {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        let mut config = crate::edge_config::load_config(None)?;
        let client = self.env.client()?;

        let (token, is_new) = acquire_ssh_token(&client, &config.config).await?;

        let host = if let Some(host) = self.host {
            host
        } else {
            // No custom host specified, use an appropriate one based on the
            // environment.
            self.env
                .app_domain()
                .context("Could not determine SSH host based on the backend url")?
        };
        let port = self.ssh_port;

        if is_new {
            eprintln!("Acquired new SSH token");
            config.set_ssh_token(token.clone())?;
            if let Err(err) = config.save() {
                eprintln!("WARNING: failed to save config: {err}");
            }
        }

        let mut cmd = std::process::Command::new("ssh");
        let mut cmd = cmd
            .args([
                // No controlpath becaue we don't want to re-use connections
                "-o",
                "ControlPath=none",
                // Disable host key checking, because we use a DNS-load-balanced
                // host.
                "-o",
                "StrictHostKeyChecking=no",
                // Don't persist known hosts - don't want to clutter users
                // regular ssh data.
                "-o",
                "UserKnownHostsFile=/dev/null",
                "-o",
                "IdentitiesOnly=yes",
                // Don't print ssh related output.
                "-q",
            ])
            .args(["-p", format!("{port}").as_str()]);
        for map_port in self.map_port {
            cmd = cmd.args(["-L", format!("{map_port}:localhost:{map_port}").as_str()]);
        }

        let package = self.run.as_deref().unwrap_or(Self::DEFAULT_PACKAGE);
        cmd = cmd.arg(format!("{token}@{host}")).arg(package);
        for run_arg in self.run_args {
            cmd = cmd.arg(&run_arg);
        }

        if self.print {
            print!("ssh");
            for arg in cmd.get_args() {
                print!(" {}", arg.to_string_lossy().as_ref());
            }
            println!();
            return Ok(());
        }

        let exit = cmd.spawn()?.wait()?;
        if exit.success() {
            Ok(())
        } else {
            Err(anyhow::anyhow!("ssh failed with status {}", exit))
        }
    }
}

type IsNew = bool;
type RawToken = String;

async fn acquire_ssh_token(
    client: &WasmerClient,
    config: &EdgeConfig,
) -> Result<(RawToken, IsNew), anyhow::Error> {
    if let Some(token) = &config.ssh_token {
        // TODO: validate that token is still valid.
        return Ok((token.clone(), false));
    }

    let token = create_ssh_token(client).await?;
    Ok((token, true))
}

/// Create a new token for SSH access through the backend API.
async fn create_ssh_token(client: &WasmerClient) -> Result<RawToken, anyhow::Error> {
    wasmer_backend_api::query::generate_deploy_config_token_raw(
        client,
        wasmer_backend_api::query::TokenKind::SSH,
    )
    .await
    .context("Could not create token for ssh access")
}
