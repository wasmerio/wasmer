use std::{net::TcpListener, path::PathBuf, str::FromStr, time::Duration};

use anyhow::Ok;

use clap::Parser;
use dialoguer::{console::style, Input};
use reqwest::Method;
use tower_http::cors::{Any, CorsLayer};

use wasmer_registry::{
    types::NewNonceOutput,
    types::ValidatedNonceOutput,
    wasmer_env::{Registry, WasmerEnv, WASMER_DIR},
    RegistryClient,
};

use axum::{extract::State, http::StatusCode, routing::post, Json, Router};

const WASMER_CLI: &str = "wasmer-cli";

/// Enum for the boolean like prompt options
#[derive(Debug, Clone, PartialEq)]
pub enum BoolPromptOptions {
    /// Signifying a yes/true - using `y/Y`
    Yes,
    /// Signifying a No/false - using `n/N`
    No,
}

impl FromStr for BoolPromptOptions {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "y" | "Y" => Ok(BoolPromptOptions::Yes),
            "n" | "N" => Ok(BoolPromptOptions::No),
            _ => Err(anyhow::anyhow!("Invalid option")),
        }
    }
}

impl ToString for BoolPromptOptions {
    fn to_string(&self) -> String {
        match self {
            BoolPromptOptions::Yes => "y".to_string(),
            BoolPromptOptions::No => "n".to_string(),
        }
    }
}

/// Subcommand for logging in using a browser
#[derive(Debug, Clone, Parser)]
pub struct Login {
    /// Variable to login without opening a browser
    #[clap(long, name = "no-browser", default_value = "false")]
    pub no_browser: bool,
    // Note: This is essentially a copy of WasmerEnv except the token is
    // accepted as a main argument instead of via --token.
    /// Set Wasmer's home directory
    #[clap(long, env = "WASMER_DIR", default_value = WASMER_DIR.as_os_str())]
    pub wasmer_dir: PathBuf,
    /// The registry to fetch packages from (inferred from the environment by
    /// default)
    #[clap(long, env = "WASMER_REGISTRY")]
    pub registry: Option<Registry>,
    /// The API token to use when communicating with the registry (inferred from
    /// the environment by default)
    pub token: Option<String>,
    /// The directory cached artefacts are saved to.
    #[clap(long, env = "WASMER_CACHE_DIR")]
    cache_dir: Option<PathBuf>,
}

impl Login {
    fn get_token_or_ask_user(&self, env: &WasmerEnv) -> Result<String, anyhow::Error> {
        if let Some(token) = &self.token {
            return Ok(token.clone());
        }

        let registry_host = env.registry_endpoint()?;
        let registry_tld = tldextract::TldExtractor::new(tldextract::TldOption::default())
            .extract(registry_host.as_str())
            .map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Invalid registry for login {}: {e}", registry_host),
                )
            })?;

        let login_prompt = match (
            registry_tld.domain.as_deref(),
            registry_tld.suffix.as_deref(),
        ) {
            (Some(d), Some(s)) => {
                format!("Please paste the login token from https://{d}.{s}/settings/access-tokens")
            }
            _ => "Please paste the login token".to_string(),
        };
        #[cfg(test)]
        {
            Ok(login_prompt)
        }
        #[cfg(not(test))]
        {
            let token = Input::new().with_prompt(&login_prompt).interact_text()?;
            Ok(token)
        }
    }

    async fn get_token_from_browser(&self, env: &WasmerEnv) -> Result<String, anyhow::Error> {
        let registry = env.registry_endpoint()?;

        let client = RegistryClient::new(registry.clone(), None, None);

        let (listener, server_url) = Self::setup_listener().await?;

        let cors_middleware = CorsLayer::new()
            .allow_headers([axum::http::header::CONTENT_TYPE])
            .allow_methods([Method::POST])
            .allow_origin(Any)
            .max_age(Duration::from_secs(60) * 10);

        let (server_shutdown_tx, mut server_shutdown_rx) = tokio::sync::mpsc::channel::<bool>(1);
        let (token_tx, mut token_rx) = tokio::sync::mpsc::channel::<String>(1);

        let app = Router::new().route(
            "/",
            post(save_validated_token).with_state((server_shutdown_tx.clone(), token_tx.clone())),
        );
        let app = app.layer(cors_middleware);

        let NewNonceOutput { auth_url, .. } =
            wasmer_registry::api::get_nonce(&client, WASMER_CLI.to_string(), server_url).await?;

        // if failed to open the browser, then don't error out just print the auth_url with a message
        println!("Opening browser at {}", &auth_url);
        opener::open_browser(&auth_url).unwrap_or_else(|_| {
            println!(
                "⚠️ Failed to open the browser.\n
                Please open the url: {}",
                &auth_url
            );
        });

        println!("\n\nWaiting for the token from the browser 🌐 ... \n");

        // start the server
        axum::Server::from_tcp(listener)?
            .serve(app.into_make_service())
            .with_graceful_shutdown(async {
                server_shutdown_rx.recv().await;
            })
            .await?;

        // receive the token from the server
        let token = token_rx
            .recv()
            .await
            .ok_or_else(|| anyhow::anyhow!("❌ Failed to receive token from localhost"))?;

        Ok(token)
    }
    fn wasmer_env(&self) -> WasmerEnv {
        WasmerEnv::new(
            self.wasmer_dir.clone(),
            self.registry.clone(),
            self.token.clone(),
            self.cache_dir.clone(),
        )
    }

    async fn setup_listener() -> Result<(TcpListener, String), anyhow::Error> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let addr = listener.local_addr()?;
        let port = addr.port();

        let server_url = format!("http://localhost:{}", port);

        eprintln!("Server URL: {}", server_url);

        Ok((listener, server_url))
    }

    /// execute [List]
    #[tokio::main]
    pub async fn execute(&self) -> Result<(), anyhow::Error> {
        let env = self.wasmer_env();
        let registry = env.registry_endpoint()?;

        let person_wants_to_login =
            match wasmer_registry::whoami(env.dir(), Some(registry.as_str()), None) {
                std::result::Result::Ok((registry, user)) => {
                    println!("You are logged in as {:?} on registry {:?}", user, registry);
                    let login_again = Input::new()
                        .with_prompt(format!(
                            "{} {} - [y/{}]",
                            style("?").yellow().bold(),
                            style("Do you want to login again?").bright().bold(),
                            style("N").green().bold()
                        ))
                        .show_default(false)
                        .default(BoolPromptOptions::No)
                        .interact_text()?;

                    login_again == BoolPromptOptions::Yes
                }
                _ => true,
            };

        if !person_wants_to_login {
            return Ok(());
        }

        let token = if self.no_browser {
            self.get_token_or_ask_user(&env)?
        } else {
            self.get_token_from_browser(&env).await?
        };

        match wasmer_registry::login::login_and_save_token(env.dir(), registry.as_str(), &token)? {
            Some(s) => println!("✅ Login for Wasmer user {:?} saved", s),
            None => println!(
                "Error: no user found on registry {:?} with token {:?}. Token saved regardless.",
                registry, token
            ),
        }
        Ok(())
    }
}

//As this function will only run once so return a Result
async fn save_validated_token(
    State((shutdown_server_tx, token_tx)): State<(
        tokio::sync::mpsc::Sender<bool>,
        tokio::sync::mpsc::Sender<String>,
    )>,
    Json(payload): Json<ValidatedNonceOutput>,
) -> StatusCode {
    let ValidatedNonceOutput { token } = payload;
    println!("Token: {}", token);

    shutdown_server_tx
        .send(true)
        .await
        .expect("Failed to send shutdown signal");

    token_tx
        .send(token.clone())
        .await
        .expect("Failed to send token");

    StatusCode::OK
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn interactive_login() {
        let temp = TempDir::new().unwrap();
        let login = Login {
            no_browser: true,
            registry: Some("wasmer.wtf".into()),
            wasmer_dir: temp.path().to_path_buf(),
            token: None,
            cache_dir: None,
        };
        let env = login.wasmer_env();

        let token = login.get_token_or_ask_user(&env).unwrap();

        assert_eq!(
            token,
            "Please paste the login token from https://wasmer.wtf/settings/access-tokens"
        );
    }

    #[test]
    fn login_with_token() {
        let temp = TempDir::new().unwrap();
        let login = Login {
            no_browser: true,
            registry: Some("wasmer.wtf".into()),
            wasmer_dir: temp.path().to_path_buf(),
            token: Some("abc".to_string()),
            cache_dir: None,
        };
        let env = login.wasmer_env();

        let token = login.get_token_or_ask_user(&env).unwrap();

        assert_eq!(token, "abc");
    }

    #[test]
    fn in_sync_with_wasmer_env() {
        let wasmer_env = WasmerEnv::command();
        let login = Login::command();

        // All options except --token should be the same
        let wasmer_env_opts: Vec<_> = wasmer_env
            .get_opts()
            .filter(|arg| arg.get_id() != "token")
            .collect();
        let login_opts: Vec<_> = login.get_opts().collect();

        assert_eq!(wasmer_env_opts, login_opts);

        // The token argument should have the same message, even if it is an
        // argument rather than a --flag.
        let wasmer_env_token_help = wasmer_env
            .get_opts()
            .find(|arg| arg.get_id() == "token")
            .unwrap()
            .get_help()
            .unwrap()
            .to_string();
        let login_token_help = login
            .get_positionals()
            .find(|arg| arg.get_id() == "token")
            .unwrap()
            .get_help()
            .unwrap()
            .to_string();
        assert_eq!(wasmer_env_token_help, login_token_help);
    }
}
