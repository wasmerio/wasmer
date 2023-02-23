use std::path::PathBuf;

use clap::Parser;
use thiserror::Error;
use wasmer::wat2wasm;

use crate::commands::Publish;

static HEADER_NO_CACHE: &'static str = r#"Cache-Control = "no-cache""#;
static HEADER_CACHE: &'static str = r#"Cache-Control = "max-age=300""#;
static HEADER_CORS: [&'static str; 3] = [
    r#"Access-Control-Allow-Origin = "*""#,
    r#"Cross-Origin-Embedder-Policy = "require-corp""#,
    r#"Cross-Origin-Opener-Policy = "same-origin""#,
];

static TEMPLATE_CONFIG_TOML: &'static str = r#"
[general]
host = "::"
port = 80
root = "/public"
log-level = "info"
cache-control-headers = false
compression = true

[advanced]

[[advanced.headers]]
source = "**"
headers = { ${HEADERS} }
"#;

static TEMPLATE_WASMER_TOML: &'static str = r#"
[package]
name = "${PCK_NAME}"
version = "${VERSION}"
description = "Container that holds the ${PCK_NAME} website."
license = "MIT"
wasmer-extra-flags = "--enable-threads --enable-bulk-memory"

[fs]
"public" = "public"
"cfg" = "cfg"

[dependencies]
"${INHERIT}" = "${INHERIT_VERSION}"

[[module]]
name = "dummy"
source = "dummy.wasm"
abi = "wasi"
"#;

static TEMPLATE_INSTRUCTIONS: &'static str = r#"

Your website was bundled with the '${INHERIT}' WASM http server and has now
been published.

To access the website from a browser use the following URL:
https://${PCK1}.${PCK2}.proxy.wapm.dev/

Note: The first time you access this website it will generate TLS encryption
      keys which can take up to a minute.

If you have a domain you would like to point to this website then create a
CNAME record that points to ${PCK1}.${PCK2}.proxy.wapm.dev.
"#;

/// CLI options for the `wasmer publish` command
#[derive(Debug, Parser)]
pub struct PublishWebSite {
    /// Registry to publish to
    #[clap(long)]
    pub registry: Option<String>,
    /// What HTTP static server to inherit from
    #[clap(long, default_value = "sharrattj/static-web-server")]
    pub inherit: String,
    /// What version of the HTTP static server to use
    #[clap(long, default_value = "1")]
    pub inherit_version: String,
    /// Override the package version of the uploaded package in the wasmer.toml
    #[clap(long)]
    pub version: Option<semver::Version>,
    /// Override the token (by default, it will use the current logged in user)
    #[clap(long)]
    pub token: Option<String>,
    /// Directory that containing the static web site
    #[clap(index = 1, name = "WEB_PATH")]
    pub web_path: String,
    /// Name of package to be uploaded to wasmer.toml
    #[clap(index = 2)]
    pub package_name: String,
    /// Run the publish logic without sending anything to the registry server
    #[clap(long, name = "dry-run")]
    pub dry_run: bool,
    /// Run the publish command without any output
    #[clap(long)]
    pub quiet: bool,
    /// Skip validation of the uploaded package
    #[clap(long)]
    pub no_validate: bool,
    /// Enables CORS which is needed for WASM modules to run
    #[clap(long)]
    pub enable_cors: bool,
    /// Indicates if the website should not do any caching
    #[clap(long)]
    pub no_web_caching: bool,
}

#[derive(Debug, Error)]
enum PublishWebSiteError {
    #[error("Unable to publish the static web site as the path \"{}\" is not valid", path.display())]
    SourceMustBeDir { path: PathBuf },
    #[error("The supplied package name is invalid")]
    PackageNameInvalid,
}

impl PublishWebSite {
    /// Executes `wasmer publish`
    pub fn execute(&self) -> Result<(), anyhow::Error> {
        // Check that the target directory is a real directory
        let web_path = PathBuf::from(self.web_path.clone());
        if std::fs::read_dir(web_path.as_path()).is_err() {
            return Err(PublishWebSiteError::SourceMustBeDir { path: web_path }.into());
        }

        // Create a random directory which we will use for constructing
        // the package before we publish it
        let tmp_dir = tempdir::TempDir::new("web_package")?;
        eprintln!(
            "Package files will be staged here: {}",
            tmp_dir.path().display()
        );

        // Generate a version number
        let version = self.version.clone().unwrap_or_else(|| {
            let version_seed = std::time::SystemTime::UNIX_EPOCH
                .elapsed()
                .unwrap()
                .as_secs();
            let version_seed = (version_seed - 1677130918) / 60;
            semver::Version::new(1, 0, version_seed)
        });
        eprintln!("Package version: {}", version);

        // Add the symbolic link to the website directory
        #[allow(deprecated)]
        std::fs::soft_link(web_path.as_path(), tmp_dir.path().join("public"))?;

        // Project the web configuration file
        std::fs::create_dir(tmp_dir.path().join("cfg"))?;
        std::fs::write(
            tmp_dir.path().join("cfg").join("config.toml"),
            self.template_substitute(TEMPLATE_CONFIG_TOML)?.as_bytes(),
        )?;

        // Write the dummy wasm module
        let wasm_bytes = wat2wasm(br#"(module)"#)?;
        std::fs::write(tmp_dir.path().join("dummy.wasm"), wasm_bytes)?;

        // Project the package file
        std::fs::write(
            tmp_dir.path().join("wasmer.toml"),
            self.template_substitute(TEMPLATE_WASMER_TOML)?.as_bytes(),
        )?;

        // Now publish the website
        let publish = Publish {
            registry: self.registry.clone(),
            token: self.token.clone(),
            dry_run: self.dry_run,
            quiet: self.quiet,
            package_name: Some(self.package_name.clone()),
            version: None,
            no_validate: self.no_validate,
            package_path: Some(tmp_dir.path().to_string_lossy().to_string()),
        };
        publish.execute()?;

        // Now we need to output what the user can do with this
        println!("{}", self.template_substitute(TEMPLATE_INSTRUCTIONS)?);

        // Close the temporary directory (destroying it)
        tmp_dir.close()?;
        Ok(())
    }

    fn version(&self) -> semver::Version {
        self.version.clone().unwrap_or_else(|| {
            let version_seed = std::time::SystemTime::UNIX_EPOCH
                .elapsed()
                .unwrap()
                .as_secs();
            let version_seed = (version_seed - 1677130918) / 60;
            semver::Version::new(1, 0, version_seed)
        })
    }

    fn template_substitute(&self, template: &str) -> Result<String, anyhow::Error> {
        let (pck2, pck1) = self
            .package_name
            .split_once("/")
            .ok_or(Into::<anyhow::Error>::into(
                PublishWebSiteError::PackageNameInvalid,
            ))?;

        let mut headers = String::new();
        if self.no_web_caching {
            headers.push_str(HEADER_NO_CACHE);
        } else {
            headers.push_str(HEADER_CACHE);
        }
        if self.enable_cors {
            HEADER_CORS.iter().for_each(|h| {
                headers.push_str(", ");
                headers.push_str(h)
            });
        }

        Ok(template
            .replace("${PCK_NAME}", &self.package_name)
            .replace("${PCK1}", pck1)
            .replace("${PCK2}", pck2)
            .replace("${VERSION}", &format!("{}", self.version()))
            .replace("${INHERIT}", &self.inherit)
            .replace("${INHERIT_VERSION}", &self.inherit_version)
            .replace("${HEADERS}", &headers))
    }
}
