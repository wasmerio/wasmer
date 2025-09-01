//! Edge SSH command.

use anyhow::Context;
use wasmer_backend_api::{types::DeployApp, WasmerClient};

use super::{app::AppIdentArgOpts, AsyncCliCommand};
use crate::{config::WasmerEnv, edge_config::LoadedEdgeConfig};

/// Start a remote SSH session.
#[derive(clap::Parser, Debug)]
pub struct CmdSsh {
    #[clap(flatten)]
    pub app: AppIdentArgOpts,

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

    /// The Edge SSH server host to connect to.
    ///
    /// You can usually ignore this flag, it mainly exists for debugging purposes.
    #[clap(long)]
    pub host: Option<String>,

    /// Prints the SSH command rather than executing it.
    #[clap(short, long)]
    pub print: bool,

    // All extra arguments.
    #[clap(trailing_var_arg = true)]
    pub extra: Vec<String>,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdSsh {
    type Output = ();

    async fn run_async(self) -> Result<(), anyhow::Error> {
        let mut config = crate::edge_config::load_config(None)?;
        let client = self.env.client()?;

        let token = acquire_ssh_token(&client, &config, &self.app).await?;
        let app_id = token.app.as_ref().map(|a| a.id.inner().to_owned());

        let host = if let Some(host) = self.host {
            host
        } else if let Some(app) = &token.app {
            // For apps, use the app domain to ensure proper routing.
            let url = app.url.clone();
            if url.starts_with("http://") || url.starts_with("https://") {
                let url = url.parse::<url::Url>().context("Could not parse app url")?;
                url.host_str()
                    .context("Could not determine host from app url")?
                    .to_string()
            } else {
                url
            }
        } else {
            // No custom host specified, use an appropriate one based on the
            // environment.
            self.env
                .app_domain()
                .context("Could not determine SSH host based on the backend url")?
        };
        let port = self.ssh_port;

        if token.is_new {
            eprintln!("Acquired new SSH token");
            config.config.add_ssh_token(app_id, token.token.clone());
            if let Err(err) = config.save() {
                eprintln!("WARNING: failed to save config: {err}");
            }
        }

        if let Some(app) = &token.app {
            eprintln!("Connecting to app '{}' at {}", app.name, host);
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

        cmd = cmd
            .arg(format!("{}@{}", token.token, host))
            .args(&self.extra);

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

type RawToken = String;

struct AcquiredToken {
    token: RawToken,
    app: Option<DeployApp>,
    is_new: bool,
}

async fn acquire_ssh_token(
    client: &WasmerClient,
    config: &LoadedEdgeConfig,
    app_opts: &AppIdentArgOpts,
) -> Result<AcquiredToken, anyhow::Error> {
    let app_opts = app_opts.clone().to_opts();

    let app = if let Some((_, app)) = app_opts.load_app_opt(client).await? {
        Some(app)
    } else {
        None
    };

    // Check cached.
    let app_id = app.as_ref().map(|a| a.id.inner());
    if let Some(token) = config.config.get_valid_ssh_token(app_id) {
        return Ok(AcquiredToken {
            token: token.to_string(),
            app,
            is_new: false,
        });
    }

    // Create new.
    let token = create_ssh_token(client, app_id.map(|x| x.to_owned())).await?;

    Ok(AcquiredToken {
        token,
        app,
        is_new: true,
    })
}

/// Create a new token for SSH access through the backend API.
async fn create_ssh_token(
    client: &WasmerClient,
    app: Option<String>,
) -> Result<RawToken, anyhow::Error> {
    wasmer_backend_api::query::generate_ssh_token(client, app)
        .await
        .context("Could not create token for ssh access")
}
