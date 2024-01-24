// Allowed because it makes code more readable.
#![allow(clippy::bool_comparison, clippy::match_like_matches_macro)]

mod types;

pub mod cmd;
pub mod config;
pub mod util;

#[cfg(all(target_os = "linux", feature = "tun-tap"))]
pub mod net;

use anyhow::Context;
use clap::Parser;
use cmd::CliCommand;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
use wasmer_api::WasmerClient;
use wasmer_registry::WasmerConfig;

pub fn run() -> Result<(), anyhow::Error> {
    let args = Args::parse();
    initialize_logging(&args);
    args.cmd.run()
}

#[derive(clap::Parser, Debug)]
#[clap(about, version)]
pub(crate) struct Args {
    #[clap(flatten)]
    pub verbosity: Option<clap_verbosity_flag::Verbosity<clap_verbosity_flag::WarnLevel>>,
    #[clap(subcommand)]
    cmd: cmd::SubCmd,
}

#[derive(clap::Parser, Debug, Clone, Default)]
pub struct ApiOpts {
    #[clap(long, env = "WASMER_TOKEN")]
    pub token: Option<String>,
    #[clap(long)]
    pub registry: Option<url::Url>,
}

struct Login {
    url: url::Url,
    token: Option<String>,
}

impl ApiOpts {
    const PROD_REGISTRY: &'static str = "https://registry.wasmer.io/graphql";

    fn load_config() -> Result<WasmerConfig, anyhow::Error> {
        let wasmer_dir = WasmerConfig::get_wasmer_dir()
            .map_err(|e| anyhow::anyhow!("no wasmer dir: '{e}' - did you run 'wasmer login'?"))?;

        let config = WasmerConfig::from_file(&wasmer_dir).map_err(|e| {
            anyhow::anyhow!("could not load config '{e}' - did you run wasmer login?")
        })?;

        Ok(config)
    }

    fn build_login(&self) -> Result<Login, anyhow::Error> {
        let config = Self::load_config()?;

        let login = if let Some(reg) = &self.registry {
            let token = if let Some(token) = &self.token {
                Some(token.clone())
            } else {
                config.registry.get_login_token_for_registry(reg.as_str())
            };

            Login {
                url: reg.clone(),
                token,
            }
        } else if let Some(login) = config.registry.current_login() {
            let url = login.registry.parse::<url::Url>().with_context(|| {
                format!(
                    "config file specified invalid registry url: '{}'",
                    login.registry
                )
            })?;

            let token = self.token.clone().unwrap_or(login.token.clone());

            Login {
                url,
                token: Some(token),
            }
        } else {
            let url = Self::PROD_REGISTRY.parse::<url::Url>().unwrap();
            Login {
                url,
                token: self.token.clone(),
            }
        };

        Ok(login)
    }

    fn client_unauthennticated(&self) -> Result<WasmerClient, anyhow::Error> {
        let login = self.build_login()?;

        let client = wasmer_api::WasmerClient::new(login.url, "edge-cli")?;

        let client = if let Some(token) = login.token {
            client.with_auth_token(token)
        } else {
            client
        };

        Ok(client)
    }

    fn client(&self) -> Result<WasmerClient, anyhow::Error> {
        let client = self.client_unauthennticated()?;
        if client.auth_token().is_none() {
            anyhow::bail!("no token provided - run 'wasmer login', specify --token=XXX, or set the WASMER_TOKEN env var");
        }

        Ok(client)
    }
}

/// Formatting options for a single item.
#[derive(clap::Parser, Debug, Default)]
pub struct ItemFormatOpts {
    /// Output format. (json, text)
    #[clap(short = 'f', long, default_value = "yaml")]
    pub format: util::render::ItemFormat,
}

/// Formatting options for a list of items.
#[derive(clap::Parser, Debug)]
pub struct ListFormatOpts {
    /// Output format. (json, text)
    #[clap(short = 'f', long, default_value = "table")]
    pub format: util::render::ListFormat,
}

/// Initialize logging.
///
/// This will prefer the `$RUST_LOG` environment variable, with the `-v` and
/// `-q` flags being used to modify the default log level.
///
/// For example, running `RUST_LOG=wasmer_registry=debug wasmer-edge -q` will
/// log everything at the `error` level (`-q` means to be one level more quiet
/// than the default `warn`), but anything from the `wasmer_registry` crate will
/// be logged at the `debug` level.
pub(crate) fn initialize_logging(args: &Args) {
    let level = args
        .verbosity
        .as_ref()
        .map(|x| x.log_level_filter())
        .unwrap_or(log::LevelFilter::Off);

    let fmt_layer = fmt::layer()
        .with_target(true)
        .with_span_events(fmt::format::FmtSpan::CLOSE)
        .with_writer(std::io::stderr)
        .compact();

    let default_level = match level {
        log::LevelFilter::Off => tracing::level_filters::LevelFilter::OFF,
        log::LevelFilter::Error => tracing::level_filters::LevelFilter::ERROR,
        log::LevelFilter::Warn => tracing::level_filters::LevelFilter::WARN,
        log::LevelFilter::Info => tracing::level_filters::LevelFilter::INFO,
        log::LevelFilter::Debug => tracing::level_filters::LevelFilter::DEBUG,
        log::LevelFilter::Trace => tracing::level_filters::LevelFilter::TRACE,
    };

    let filter_layer = EnvFilter::builder()
        .with_default_directive(default_level.into())
        .from_env_lossy();

    tracing_subscriber::registry()
        .with(filter_layer)
        .with(fmt_layer)
        .init();
}
