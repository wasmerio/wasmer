use clap::Parser;
#[cfg(not(test))]
use dialoguer::Input;

/// Subcommand for listing packages
#[derive(Debug, Clone, Parser)]
pub struct Login {
    /// Registry to log into (default: wapm.io)
    #[clap(long, default_value = "wapm.io")]
    pub registry: String,
    /// Login token
    #[clap(name = "TOKEN")]
    pub token: Option<String>,
}

impl Login {
    fn get_token_or_ask_user(&self) -> Result<String, std::io::Error> {
        match self.token.as_ref() {
            Some(s) => Ok(s.clone()),
            None => {
                let registry_host = wasmer_registry::format_graphql(&self.registry);
                let registry_tld = tldextract::TldExtractor::new(tldextract::TldOption::default())
                    .extract(&registry_host)
                    .map_err(|e| {
                        std::io::Error::new(
                            std::io::ErrorKind::Other,
                            format!("Invalid registry for login {}: {e}", self.registry),
                        )
                    })?;
                let login_prompt = match (
                    registry_tld.domain.as_deref(),
                    registry_tld.suffix.as_deref(),
                ) {
                    (Some(d), Some(s)) => {
                        format!("Please paste the login token for https://{d}.{s}/me")
                    }
                    _ => "Please paste the login token".to_string(),
                };
                #[cfg(test)]
                {
                    Ok(login_prompt)
                }
                #[cfg(not(test))]
                {
                    Input::new().with_prompt(&login_prompt).interact_text()
                }
            }
        }
    }

    /// execute [List]
    pub fn execute(&self) -> Result<(), anyhow::Error> {
        let token = self.get_token_or_ask_user()?;
        match wasmer_registry::login::login_and_save_token(&self.registry, &token)? {
            Some(s) => println!("Login for WAPM user {:?} saved", s),
            None => println!(
                "Error: no user found on registry {:?} with token {:?}. Token saved regardless.",
                self.registry, token
            ),
        }
        Ok(())
    }
}

#[test]
fn test_login_2() {
    let login = Login {
        registry: "wapm.dev".to_string(),
        token: None,
    };

    assert_eq!(
        login.get_token_or_ask_user().unwrap(),
        "Please paste the login token for https://wapm.dev/me"
    );

    let login = Login {
        registry: "wapm.dev".to_string(),
        token: Some("abc".to_string()),
    };

    assert_eq!(login.get_token_or_ask_user().unwrap(), "abc");
}
