mod auth_server;
use auth_server::*;
use colored::Colorize;
use hyper::{server::conn::http1::Builder, service::service_fn};
use hyper_util::server::graceful::GracefulShutdown;

use crate::{
    commands::AsyncCliCommand,
    config::{UpdateRegistry, UserRegistry, WasmerConfig, WasmerEnv},
};
use futures_util::{stream::FuturesUnordered, StreamExt};
use std::{path::PathBuf, time::Duration};
use wasmer_backend_api::{types::Nonce, WasmerClient};

#[derive(Debug, Clone)]
enum AuthorizationState {
    TokenSuccess(String),
    Cancelled,
    TimedOut,
    UnknownMethod,
}

/// Subcommand for log in a user into Wasmer (using a browser or provided a token)
#[derive(Debug, Clone, clap::Parser)]
pub struct Login {
    /// Variable to login without opening a browser
    #[clap(long, name = "no-browser", default_value = "false")]
    pub no_browser: bool,

    // This is a copy of [`WasmerEnv`] to allow users to specify
    // the token as a parameter rather than as a flag.
    /// Set Wasmer's home directory
    #[clap(long, env = "WASMER_DIR", default_value = crate::config::DEFAULT_WASMER_DIR.as_os_str())]
    pub wasmer_dir: PathBuf,

    /// The directory cached artefacts are saved to.
    #[clap(long, env = "WASMER_CACHE_DIR", default_value = crate::config::DEFAULT_WASMER_CACHE_DIR.as_os_str())]
    pub cache_dir: PathBuf,

    /// The API token to use when communicating with the registry (inferred from the environment by default)
    #[clap(env = "WASMER_TOKEN")]
    pub token: Option<String>,

    /// Change the current registry
    #[clap(long, env = "WASMER_REGISTRY")]
    pub registry: Option<UserRegistry>,
}

impl Login {
    fn get_token_from_env_or_user(
        &self,
        env: &WasmerEnv,
    ) -> Result<AuthorizationState, anyhow::Error> {
        if let Some(token) = &self.token {
            return Ok(AuthorizationState::TokenSuccess(token.clone()));
        }

        let public_url = env.registry_public_url()?;

        let login_prompt = match public_url.domain() {
            Some(d) => {
                format!("Please paste the login token from https://{d}/settings/access-tokens")
            }
            _ => "Please paste the login token".to_string(),
        };

        #[cfg(test)]
        {
            Ok(AuthorizationState::TokenSuccess(login_prompt))
        }
        #[cfg(not(test))]
        {
            let token = dialoguer::Input::new()
                .with_prompt(&login_prompt)
                .interact_text()?;
            Ok(AuthorizationState::TokenSuccess(token))
        }
    }

    async fn get_token_from_browser(
        &self,
        client: &WasmerClient,
    ) -> anyhow::Result<AuthorizationState> {
        let (listener, server_url) = setup_listener().await?;

        let (server_shutdown_tx, mut server_shutdown_rx) = tokio::sync::mpsc::channel::<bool>(1);
        let (token_tx, mut token_rx) = tokio::sync::mpsc::channel::<AuthorizationState>(1);

        // Create a new AppContext
        let app_context = BrowserAuthContext {
            server_shutdown_tx,
            token_tx,
        };

        let Nonce { auth_url, .. } =
            wasmer_backend_api::query::create_nonce(client, "wasmer-cli".to_string(), server_url)
                .await?
                .ok_or_else(|| {
                    anyhow::anyhow!("The backend did not return any nonce to auth the login!")
                })?;

        // if failed to open the browser, then don't error out just print the auth_url with a message
        println!("Opening auth link in your default browser: {}", &auth_url);
        opener::open_browser(&auth_url).unwrap_or_else(|_| {
            println!(
                "⚠️ Failed to open the browser.\n
            Please open the url: {}",
                &auth_url
            );
        });

        // Jump through hyper 1.0's hoops...
        let graceful = GracefulShutdown::new();

        let http = Builder::new();

        let mut futs = FuturesUnordered::new();

        let service = service_fn(move |req| service_router(app_context.clone(), req));

        print!("Waiting for session... ");

        // start the server
        loop {
            tokio::select! {
                Result::Ok((stream, _addr)) = listener.accept() => {
                    let io = hyper_util::rt::tokio::TokioIo::new(stream);
                    let conn = http.serve_connection(io, service.clone());
                    // watch this connection
                    let fut = graceful.watch(conn);
                    futs.push(async move {
                        if let Err(e) = fut.await {
                            eprintln!("Error serving connection: {e:?}");
                        }
                    });
                },

                _ = futs.next() => {}

                _ = server_shutdown_rx.recv() => {
                    // stop the accept loop
                    break;
                }
            }
        }

        // receive the token from the server
        let token = token_rx
            .recv()
            .await
            .ok_or_else(|| anyhow::anyhow!("❌ Failed to receive token from localhost"))?;

        Ok(token)
    }

    async fn do_login(&self, env: &WasmerEnv) -> anyhow::Result<AuthorizationState> {
        let client = env.client_unauthennticated()?;

        let should_login =
            if let Some(user) = wasmer_backend_api::query::current_user(&client).await? {
                #[cfg(not(test))]
                {
                    println!(
                        "You are already logged in as {} in registry {}.",
                        user.username.bold(),
                        env.registry_public_url()?.host_str().unwrap().bold()
                    );
                    let theme = dialoguer::theme::ColorfulTheme::default();
                    let dialog = dialoguer::Confirm::with_theme(&theme).with_prompt("Login again?");

                    dialog.interact()?
                }
                #[cfg(test)]
                {
                    // prevent unused binding warning
                    _ = user;

                    false
                }
            } else {
                true
            };

        if !should_login {
            Ok(AuthorizationState::Cancelled)
        } else if self.no_browser {
            self.get_token_from_env_or_user(env)
        } else {
            // switch between two methods of getting the token.
            // start two async processes, 10 minute timeout and get token from browser. Whichever finishes first, use that.
            let timeout_future = tokio::time::sleep(Duration::from_secs(60 * 10));
            tokio::select! {
             _ = timeout_future => {
                     Ok(AuthorizationState::TimedOut)
                 },
                 token = self.get_token_from_browser(&client) => {
                    token
                 }
            }
        }
    }

    async fn login_and_save(&self, env: &WasmerEnv, token: String) -> anyhow::Result<String> {
        let registry = env.registry_endpoint()?;
        let mut config = WasmerConfig::from_file(env.dir())
            .map_err(|e| anyhow::anyhow!("config from file: {e}"))?;
        config
            .registry
            .set_current_registry(registry.as_ref())
            .await;
        config.registry.set_login_token_for_registry(
            &config.registry.get_current_registry(),
            &token,
            UpdateRegistry::Update,
        );
        let path = WasmerConfig::get_file_location(env.dir());
        config.save(path)?;

        // This will automatically read the config again, picking up the new edits.
        let client = env.client()?;

        wasmer_backend_api::query::current_user(&client)
            .await?
            .map(|v| v.username)
            .ok_or_else(|| anyhow::anyhow!("Not logged in!"))
    }

    pub(crate) fn get_wasmer_env(&self) -> WasmerEnv {
        WasmerEnv::new(
            self.wasmer_dir.clone(),
            self.cache_dir.clone(),
            self.token.clone(),
            self.registry.clone(),
        )
    }
}

#[async_trait::async_trait]
impl AsyncCliCommand for Login {
    type Output = ();

    async fn run_async(self) -> Result<Self::Output, anyhow::Error> {
        let env = self.get_wasmer_env();

        let auth_state = match &self.token {
            Some(token) => AuthorizationState::TokenSuccess(token.clone()),
            None => self.do_login(&env).await?,
        };

        match auth_state {
            AuthorizationState::TokenSuccess(token) => {
                match self.login_and_save(&env, token).await {
                    Ok(s) => {
                        print!("Done!");
                        println!("\n{} Login for Wasmer user {:?} saved","✔".green().bold(), s)
                    }
                    Err(_) => print!(
                        "Warning: no user found on {:?} with the provided token.\nToken saved regardless.",
                        env.registry_public_url()
                    ),
                }
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

#[cfg(test)]
mod tests {
    use clap::CommandFactory;
    use tempfile::TempDir;

    use crate::commands::CliCommand;

    use super::*;

    #[test]
    fn interactive_login() {
        let temp = TempDir::new().unwrap();
        let login = Login {
            no_browser: true,
            registry: Some("wasmer.wtf".into()),
            wasmer_dir: temp.path().to_path_buf(),
            token: None,
            cache_dir: temp.path().join("cache").to_path_buf(),
        };
        let env = login.get_wasmer_env();

        let token = login.get_token_from_env_or_user(&env).unwrap();
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
            cache_dir: temp.path().join("cache").to_path_buf(),
        };
        let env = login.get_wasmer_env();

        let token = login.get_token_from_env_or_user(&env).unwrap();

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
            wasmer_dir: crate::config::DEFAULT_WASMER_DIR.clone(),
            registry: Some("http://localhost:11".to_string().into()),
            token: Some("invalid".to_string()),
            cache_dir: crate::config::DEFAULT_WASMER_CACHE_DIR.clone(),
        };

        let res = cmd.run();
        // The CLI notices that either the registry is unreachable or the token is not tied to any
        // user. It shows a warning to the user, but does not return with an error code.
        //
        //  ------ i.e. this will fail
        // |
        // v
        // assert!(res.is_err());
        assert!(res.is_ok());
    }
}
