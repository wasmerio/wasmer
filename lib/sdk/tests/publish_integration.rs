use std::fs;
use std::time::Duration;

use rand::Rng as _;
use semver::Version;
use tempfile::tempdir;
use url::Url;

use wasmer_backend_api::WasmerClient;

use wasmer_sdk::package::publish::{PublishOptions, PublishWait, publish_package_directory};

/// Integration test for the package publishing logic.
#[tokio::test]
#[ignore]
async fn publish_package_integration() -> anyhow::Result<()> {
    // Use provided registry and token credentials.
    let registry = std::env::var("WASMER_REGISTRY")
        .unwrap_or_else(|_| "https://registry.wasmer.wtf/graphql".to_string());
    let token = std::env::var("WASMER_TOKEN").unwrap();

    // Construct the API client.
    let registry_url = Url::parse(&registry)?;
    let client = WasmerClient::new(registry_url, "sdk-integration-test")?.with_auth_token(token);

    // Prepare a temporary directory for the package.
    let tempdir = tempdir()?;
    let path = tempdir.path();

    let user = wasmer_backend_api::query::current_user(&client)
        .await?
        .expect("no current user");

    // Generate a random number
    let name = format!("t-{:010}", rand::rng().random_range(0u64..9999999999));

    let manifest = r#"
[dependencies]
"wasmer/static-web-server" = "^1"

[fs]
"/public" = "public"

[[command]]
name = "webserver"
module = "wasmer/static-web-server:webserver"
runner = "https://webc.org/runner/wasi"

[command.annotations.wasi]
main-args = ["--root", "/public"]
"#;

    fs::write(path.join("wasmer.toml"), manifest)?;

    std::fs::create_dir(path.join("public"))?;
    fs::write(path.join("public/index.html"), "<h1>Hello, World!</h1>")?;

    // Configure publishing options.
    let opts = PublishOptions {
        namespace: Some(user.username.clone()),
        name: Some(name),
        version: Some(Version::parse("0.0.1")?),
        timeout: Duration::from_secs(180),
        wait: PublishWait::None,
        walker_factory: wasmer_package::package::wasmer_ignore_walker(),
    };

    // Execute the publish logic.
    publish_package_directory(&client, path, opts, |_| {}).await?;
    Ok(())
}
