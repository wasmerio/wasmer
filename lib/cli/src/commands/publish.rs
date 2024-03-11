use anyhow::Context as _;
use clap::Parser;
use wasmer_registry::{publish::PublishWait, wasmer_env::WasmerEnv};

use super::PackageBuild;

/// Publish a package to the package registry.
#[derive(Debug, Parser)]
pub struct Publish {
    #[clap(flatten)]
    env: WasmerEnv,
    /// Run the publish logic without sending anything to the registry server
    #[clap(long, name = "dry-run")]
    pub dry_run: bool,
    /// Run the publish command without any output
    #[clap(long)]
    pub quiet: bool,
    /// Override the package of the uploaded package in the wasmer.toml
    #[clap(long)]
    pub package_name: Option<String>,
    /// Override the package version of the uploaded package in the wasmer.toml
    #[clap(long)]
    pub version: Option<semver::Version>,
    /// Skip validation of the uploaded package
    #[clap(long)]
    pub no_validate: bool,
    /// Directory containing the `wasmer.toml`, or a custom *.toml manifest file.
    ///
    /// Defaults to current working directory.
    #[clap(name = "PACKAGE_PATH")]
    pub package_path: Option<String>,
    /// Wait for package to be available on the registry before exiting.
    #[clap(long)]
    pub wait: bool,
    /// Wait for the package and all dependencies to be available on the registry
    /// before exiting.
    ///
    /// This includes the container, native executables and bindings.
    #[clap(long)]
    pub wait_all: bool,
    /// Timeout (in seconds) for the publish query to the registry.
    ///
    /// Note that this is not the timeout for the entire publish process, but
    /// for each individual query to the registry during the publish flow.
    #[clap(long, default_value = "2m")]
    pub timeout: humantime::Duration,
}

impl Publish {
    /// Executes `wasmer publish`
    pub fn execute(&self) -> Result<(), anyhow::Error> {
        // first check if the package could be built successfuly
        let package_path = match self.package_path.as_ref() {
            Some(s) => std::env::current_dir()?.join(s),
            None => std::env::current_dir()?,
        };
        PackageBuild::check(package_path).execute()?;

        let token = self
            .env
            .token()
            .context("could not determine auth token for registry - run 'wasmer login'")?;

        let wait = if self.wait_all {
            PublishWait::new_all()
        } else if self.wait {
            PublishWait::new_container()
        } else {
            PublishWait::new_none()
        };

        let publish = wasmer_registry::package::builder::Publish {
            registry: self.env.registry_endpoint().map(|u| u.to_string()).ok(),
            dry_run: self.dry_run,
            quiet: self.quiet,
            package_name: self.package_name.clone(),
            version: self.version.clone(),
            token,
            no_validate: self.no_validate,
            package_path: self.package_path.clone(),
            wait,
            timeout: self.timeout.into(),
        };
        publish.execute().map_err(on_error)?;

        if let Err(e) = invalidate_graphql_query_cache(&self.env) {
            tracing::warn!(
                error = &*e,
                "Unable to invalidate the cache used for package version queries",
            );
        }

        Ok(())
    }
}

fn on_error(e: anyhow::Error) -> anyhow::Error {
    #[cfg(feature = "telemetry")]
    sentry::integrations::anyhow::capture_anyhow(&e);

    e
}

// HACK: We want to invalidate the cache used for GraphQL queries so
// the current user sees the results of publishing immediately. There
// are cleaner ways to achieve this, but for now we're just going to
// clear out the whole GraphQL query cache.
// See https://github.com/wasmerio/wasmer/pull/3983 for more
fn invalidate_graphql_query_cache(env: &WasmerEnv) -> Result<(), anyhow::Error> {
    let cache_dir = env.cache_dir().join("queries");
    std::fs::remove_dir_all(cache_dir)?;

    Ok(())
}
