use clap::Parser;

/// Publish a package to the package registry.
#[derive(Debug, Parser)]
pub struct Publish {
    /// Registry to publish to
    #[clap(long)]
    pub registry: Option<String>,
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
    /// Override the token (by default, it will use the current logged in user)
    #[clap(long)]
    pub token: Option<String>,
    /// Skip validation of the uploaded package
    #[clap(long)]
    pub no_validate: bool,
    /// Directory containing the `wasmer.toml` (defaults to current root dir)
    #[clap(name = "PACKAGE_PATH")]
    pub package_path: Option<String>,
}

impl Publish {
    /// Executes `wasmer publish`
    pub fn execute(&self) -> Result<(), anyhow::Error> {
        let publish = wasmer_registry::package::builder::Publish {
            registry: self.registry.clone(),
            dry_run: self.dry_run,
            quiet: self.quiet,
            package_name: self.package_name.clone(),
            version: self.version.clone(),
            token: self.token.clone(),
            no_validate: self.no_validate,
            package_path: self.package_path.clone(),
        };
        publish.execute().map_err(on_error)
    }
}

fn on_error(e: anyhow::Error) -> anyhow::Error {
    #[cfg(feature = "telemetry")]
    sentry::integrations::anyhow::capture_anyhow(&e);

    e
}
