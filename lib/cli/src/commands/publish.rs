use clap::Parser;
use wasmer_registry::wasmer_env::WasmerEnv;

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
}

impl Publish {
    /// Executes `wasmer publish`
    pub fn execute(&self) -> Result<(), anyhow::Error> {
        let publish = wasmer_registry::package::builder::Publish {
            registry: self.env.registry_endpoint().map(|u| u.to_string()).ok(),
            dry_run: self.dry_run,
            quiet: self.quiet,
            package_name: self.package_name.clone(),
            version: self.version.clone(),
            token: self.env.token(),
            no_validate: self.no_validate,
            package_path: self.package_path.clone(),
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
