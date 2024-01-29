use anyhow::Context;
use colored::Colorize;
use dialoguer::Select;
use edge_schema::schema::StringWebcIdent;
use wasmer_api::WasmerClient;

pub fn prompt_for_ident(message: &str, default: Option<&str>) -> Result<String, anyhow::Error> {
    loop {
        let diag = dialoguer::Input::new()
            .with_prompt(message)
            .with_initial_text(default.unwrap_or_default());

        // if let Some(val) = &validator {
        //     diag.validate_with(val);
        // }

        let raw: String = diag.interact()?;
        let val = raw.trim();
        if !val.is_empty() {
            break Ok(val.to_string());
        }
    }
}

/// Ask a user for a package name.
///
/// Will continue looping until the user provides a valid name.
pub fn prompt_for_package_ident(
    message: &str,
    default: Option<&str>,
) -> Result<StringWebcIdent, anyhow::Error> {
    loop {
        let raw: String = dialoguer::Input::new()
            .with_prompt(message)
            .with_initial_text(default.unwrap_or_default())
            .interact_text()
            .context("could not read user input")?;

        match raw.parse::<StringWebcIdent>() {
            Ok(p) => break Ok(p),
            Err(err) => {
                eprintln!("invalid package name: {err}");
            }
        }
    }
}

/// Defines how to check for a package.
pub enum PackageCheckMode {
    /// The package must exist in the registry.
    MustExist,
    /// The package must NOT exist in the registry.
    #[allow(dead_code)]
    MustNotExist,
}

/// Ask for a package name.
///
/// Will continue looping until the user provides a valid name.
///
/// If an API is provided, will check if the package exists.
pub async fn prompt_for_package(
    message: &str,
    default: Option<&str>,
    check: Option<PackageCheckMode>,
    client: Option<&WasmerClient>,
) -> Result<(StringWebcIdent, Option<wasmer_api::types::Package>), anyhow::Error> {
    loop {
        let ident = prompt_for_package_ident(message, default)?;

        if let Some(check) = &check {
            let api = client.expect("Check mode specified, but no API provided");

            let pkg = wasmer_api::query::get_package(api, ident.to_string())
                .await
                .context("could not query backend for package")?;

            match check {
                PackageCheckMode::MustExist => {
                    if let Some(pkg) = pkg {
                        let mut ident = ident;
                        if let Some(v) = &pkg.last_version {
                            ident.0.tag = Some(v.version.clone());
                        }
                        break Ok((ident, Some(pkg)));
                    } else {
                        eprintln!("Package '{ident}' does not exist");
                    }
                }
                PackageCheckMode::MustNotExist => {
                    if pkg.is_none() {
                        break Ok((ident, None));
                    } else {
                        eprintln!("Package '{ident}' already exists");
                    }
                }
            }
        }
    }
}

/// Prompt for a namespace.
///
/// Will either show a select with all available namespaces based on the `user`
/// argument, or present a basic text input.
///
/// The username will be included as an option.
pub fn prompt_for_namespace(
    message: &str,
    default: Option<&str>,
    user: Option<&wasmer_api::types::UserWithNamespaces>,
) -> Result<String, anyhow::Error> {
    if let Some(user) = user {
        let namespaces = user
            .namespaces
            .edges
            .clone()
            .into_iter()
            .flatten()
            .filter_map(|e| e.node)
            .collect::<Vec<_>>();

        let labels = [user.username.clone()]
            .into_iter()
            .chain(namespaces.iter().map(|ns| ns.global_name.clone()))
            .collect::<Vec<_>>();

        let selection_index = Select::new()
            .with_prompt(message)
            .default(0)
            .items(&labels)
            .interact()
            .context("could not read user input")?;

        Ok(labels[selection_index].clone())
    } else {
        loop {
            let value = dialoguer::Input::<String>::new()
                .with_prompt(message)
                .with_initial_text(default.map(|x| x.trim().to_string()).unwrap_or_default())
                .interact_text()
                .context("could not read user input")?
                .trim()
                .to_string();

            if !value.is_empty() {
                break Ok(value);
            }
        }
    }
}

/// Prompt for an app name.
/// If an api provided, will check if an app with the givne alias already exists.
#[allow(dead_code)]
pub async fn prompt_new_app_name(
    message: &str,
    default: Option<&str>,
    namespace: &str,
    api: Option<&WasmerClient>,
) -> Result<String, anyhow::Error> {
    loop {
        let ident = prompt_for_ident(message, default)?;

        if let Some(api) = &api {
            let app = wasmer_api::query::get_app(api, namespace.to_string(), ident.clone()).await?;
            eprintln!("Checking name availability...");
            if app.is_some() {
                eprintln!(
                    "{}: app '{}/{}' already exists - pick a different name",
                    "WARN:".yellow(),
                    namespace,
                    ident
                );
            } else {
                break Ok(ident);
            }
        }
    }
}

/// Prompt for an app name.
/// If an api provided, will check if an app with the givne alias already exists.
#[allow(dead_code)]
pub async fn prompt_new_app_alias(
    message: &str,
    default: Option<&str>,
    api: Option<&WasmerClient>,
) -> Result<String, anyhow::Error> {
    loop {
        let ident = prompt_for_ident(message, default)?;

        if let Some(api) = &api {
            let app = wasmer_api::query::get_app_by_alias(api, ident.clone()).await?;
            eprintln!("Checking name availability...");
            if app.is_some() {
                eprintln!(
                    "{}: alias '{}' already exists - pick a different name",
                    "WARN:".yellow(),
                    ident
                );
            } else {
                break Ok(ident);
            }
        }
    }
}
