use std::{net::TcpListener, path::PathBuf, str::FromStr, time::Duration};

use anyhow::Ok;
use clap::Parser;
#[cfg(not(test))]
use dialoguer::{console::style, Input};
use hyper::{
    service::{make_service_fn, service_fn},
    Body, Request, Response, Server, StatusCode,
};
use reqwest::Method;
use serde::Deserialize;
use wasmer_registry::{
    types::NewNonceOutput,
    wasmer_env::{Registry, WasmerEnv, WASMER_DIR},
    RegistryClient,
};

const WASMER_CLI: &str = "wasmer-cli";

/// Payload from the frontend after the user has authenticated.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TokenStatus {
    /// Signifying that the token is cancelled
    Cancelled,
    /// Signifying that the token is authorized
    Authorized,
}

/// Payload from the frontend after the user has authenticated.
///
/// This has the token that we need to set in the WASMER_TOML file.
#[derive(Clone, Debug, Deserialize)]
pub struct ValidatedNonceOutput {
    /// Token Received from the frontend
    pub token: Option<String>,
    /// Status of the token , whether it is authorized or cancelled
    pub status: TokenStatus,
}

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

type Token = String;

#[derive(Debug, Clone)]
enum AuthorizationState {
    TokenSuccess(Token),
    Cancelled,
    TimedOut,
    UnknownMethod,
}

#[derive(Clone)]
struct AppContext {
    server_shutdown_tx: tokio::sync::mpsc::Sender<bool>,
    token_tx: tokio::sync::mpsc::Sender<AuthorizationState>,
}

/// Subcommand for log in a user into Wasmer (using a browser or provided a token)
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
    fn get_token_or_ask_user(&self, env: &WasmerEnv) -> Result<AuthorizationState, anyhow::Error> {
        if let Some(token) = &self.token {
            return Ok(AuthorizationState::TokenSuccess(token.clone()));
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
            Ok(AuthorizationState::TokenSuccess(login_prompt))
        }
        #[cfg(not(test))]
        {
            let token = Input::new().with_prompt(&login_prompt).interact_text()?;
            Ok(AuthorizationState::TokenSuccess(token))
        }
    }

    async fn get_token_from_browser(
        &self,
        env: &WasmerEnv,
    ) -> Result<AuthorizationState, anyhow::Error> {
        let registry = env.registry_endpoint()?;

        let client = RegistryClient::new(registry.clone(), None, None);

        let (listener, server_url) = Self::setup_listener().await?;

        let (server_shutdown_tx, mut server_shutdown_rx) = tokio::sync::mpsc::channel::<bool>(1);
        let (token_tx, mut token_rx) = tokio::sync::mpsc::channel::<AuthorizationState>(1);

        // Create a new AppContext
        let app_context = AppContext {
            server_shutdown_tx,
            token_tx,
        };

        let NewNonceOutput { auth_url } =
            wasmer_registry::api::create_nonce(&client, WASMER_CLI.to_string(), server_url).await?;

        // if failed to open the browser, then don't error out just print the auth_url with a message
        println!("Opening auth link in your default browser: {}", &auth_url);
        opener::open_browser(&auth_url).unwrap_or_else(|_| {
            println!(
                "⚠️ Failed to open the browser.\n
            Please open the url: {}",
                &auth_url
            );
        });

        // Create a new server
        let make_svc = make_service_fn(move |_| {
            let context = app_context.clone();

            // Create a `Service` for responding to the request.
            let service = service_fn(move |req| service_router(context.clone(), req));

            // Return the service to hyper.
            async move { Ok(service) }
        });

        print!("Waiting for session... ");

        // start the server
        Server::from_tcp(listener)?
            .serve(make_svc)
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

        Ok((listener, server_url))
    }

    /// execute [List]
    #[tokio::main]
    pub async fn execute(&self) -> Result<(), anyhow::Error> {
        let env = self.wasmer_env();
        let registry = env.registry_endpoint()?;

        let auth_state = match self.token.clone() {
            Some(token) => Ok(AuthorizationState::TokenSuccess(token)),
            None => {
                let person_wants_to_login =
                    match wasmer_registry::whoami(env.dir(), Some(registry.as_str()), None) {
                        std::result::Result::Ok((registry, user)) => {
                            println!(
                                "You are already logged in as {:?} on registry {:?}",
                                user, registry
                            );

                            #[cfg(not(test))]
                            {
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
                            #[cfg(test)]
                            {
                                false
                            }
                        }
                        _ => true,
                    };

                if !person_wants_to_login {
                    Ok(AuthorizationState::Cancelled)
                } else if self.no_browser {
                    self.get_token_or_ask_user(&env)
                } else {
                    // switch between two methods of getting the token.
                    // start two async processes, 10 minute timeout and get token from browser. Whichever finishes first, use that.
                    let timeout_future = tokio::time::sleep(Duration::from_secs(60 * 10));
                    tokio::select! {
                     _ = timeout_future => {
                             Ok(AuthorizationState::TimedOut)
                         },
                         token = self.get_token_from_browser(&env) => {
                            token
                         }
                    }
                }
            }
        }?;

        match auth_state {
            AuthorizationState::TokenSuccess(token) => {
                let res = std::thread::spawn({
                    let dir = env.dir().to_owned();
                    let registry = registry.clone();
                    move || {
                        wasmer_registry::login::login_and_save_token(
                            &dir,
                            registry.as_str(),
                            &token,
                        )
                    }
                })
                .join()
                .map_err(|err| anyhow::format_err!("handler thread died: {err:?}"))??;

                match res {
                    Some(s) => {
                        print!("Done!");
                        println!("\n✅ Login for Wasmer user {:?} saved", s)
                    }
                    None => print!(
                        "Warning: no user found on {:?} with the provided token.\nToken saved regardless.",
                        registry.domain().unwrap_or("registry.wasmer.io")
                    ),
                };
            }
            AuthorizationState::TimedOut => {
                print!("Timed out (10 mins exceeded)");
            }
            AuthorizationState::Cancelled => {
                println!("Cancelled by the user");
            }
            AuthorizationState::UnknownMethod => {
                println!("Error: unknown method\n");
            }
        };
        Ok(())
    }
}

async fn preflight(req: Request<Body>) -> Result<Response<Body>, anyhow::Error> {
    let _whole_body = hyper::body::aggregate(req).await?;
    let response = Response::builder()
        .status(StatusCode::OK)
        .header("Access-Control-Allow-Origin", "*") // FIXME: this is not secure, Don't allow all origins. @syrusakbary
        .header("Access-Control-Allow-Headers", "Content-Type")
        .header("Access-Control-Allow-Methods", "POST, OPTIONS")
        .body(Body::default())?;
    Ok(response)
}

async fn handle_post_save_token(
    context: AppContext,
    req: Request<Body>,
) -> Result<Response<Body>, anyhow::Error> {
    let AppContext {
        server_shutdown_tx,
        token_tx,
    } = context;
    let (.., body) = req.into_parts();
    let body = hyper::body::to_bytes(body).await?;

    let ValidatedNonceOutput {
        token,
        status: token_status,
    } = serde_json::from_slice::<ValidatedNonceOutput>(&body)?;

    // send the AuthorizationState based on token_status to the main thread and get the response message
    let (response_message, parse_failure) = match token_status {
        TokenStatus::Cancelled => {
            token_tx
                .send(AuthorizationState::Cancelled)
                .await
                .expect("Failed to send token");

            ("Token Cancelled by the user", false)
        }
        TokenStatus::Authorized => {
            if let Some(token) = token {
                token_tx
                    .send(AuthorizationState::TokenSuccess(token.clone()))
                    .await
                    .expect("Failed to send token");
                ("Token Authorized", false)
            } else {
                ("Token not found", true)
            }
        }
    };

    server_shutdown_tx
        .send(true)
        .await
        .expect("Failed to send shutdown signal");

    let status = if parse_failure {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::OK
    };

    Ok(Response::builder()
        .status(status)
        .header("Access-Control-Allow-Origin", "*") // FIXME: this is not secure, Don't allow all origins. @syrusakbary
        .header("Access-Control-Allow-Headers", "Content-Type")
        .header("Access-Control-Allow-Methods", "POST, OPTIONS")
        .body(Body::from(response_message))?)
}

async fn handle_unknown_method(context: AppContext) -> Result<Response<Body>, anyhow::Error> {
    let AppContext {
        server_shutdown_tx,
        token_tx,
    } = context;

    token_tx
        .send(AuthorizationState::UnknownMethod)
        .await
        .expect("Failed to send token");

    server_shutdown_tx
        .send(true)
        .await
        .expect("Failed to send shutdown signal");

    Ok(Response::builder()
        .status(StatusCode::METHOD_NOT_ALLOWED)
        .body(Body::from("Method not allowed"))?)
}

/// Handle the preflight headers first - OPTIONS request
/// Then proceed to handle the actual request - POST request
async fn service_router(
    context: AppContext,
    req: Request<Body>,
) -> Result<Response<Body>, anyhow::Error> {
    match *req.method() {
        Method::OPTIONS => preflight(req).await,
        Method::POST => handle_post_save_token(context, req).await,
        _ => handle_unknown_method(context).await,
    }
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
        match token {
            AuthorizationState::TokenSuccess(token) => {
                assert_eq!(
                    token,
                    "Please paste the login token from https://wasmer.wtf/settings/access-tokens"
                );
            }
            AuthorizationState::Cancelled
            | AuthorizationState::TimedOut
            | AuthorizationState::UnknownMethod => {
                panic!("Should not reach here")
            }
        }
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

        match token {
            AuthorizationState::TokenSuccess(token) => {
                assert_eq!(token, "abc");
            }
            AuthorizationState::Cancelled
            | AuthorizationState::TimedOut
            | AuthorizationState::UnknownMethod => {
                panic!("Should not reach here")
            }
        }
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

    /// Regression test for panics on API errors.
    /// See https://github.com/wasmerio/wasmer/issues/4147.
    #[test]
    fn login_with_invalid_token_does_not_panic() {
        let cmd = Login {
            no_browser: true,
            wasmer_dir: WASMER_DIR.clone(),
            registry: Some("http://localhost:11".to_string().into()),
            token: Some("invalid".to_string()),
            cache_dir: None,
        };

        let res = cmd.execute();
        assert!(res.is_err());
    }
}
