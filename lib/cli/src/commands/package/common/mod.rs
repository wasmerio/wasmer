use crate::{
    commands::{AsyncCliCommand, Login},
    config::WasmerEnv,
    utils::load_package_manifest,
};
use anyhow::Context;
use bytes::Bytes;
use colored::Colorize;
use dialoguer::Confirm;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Body;
use sha2::Digest;
use std::path::{Path, PathBuf};
use wasmer_backend_api::{WasmerClient, query::UploadMethod};
use wasmer_config::package::{Manifest, NamedPackageIdent, PackageHash, Webcm};
use wasmer_package::utils::{from_bytes, from_disk};

pub mod macros;
pub mod wait;

pub(super) fn on_error(e: anyhow::Error) -> anyhow::Error {
    #[cfg(feature = "telemetry")]
    sentry::integrations::anyhow::capture_anyhow(&e);

    e
}

// HACK: We want to invalidate the cache used for GraphQL queries so
// the current user sees the results of publishing immediately. There
// are cleaner ways to achieve this, but for now we're just going to
// clear out the whole GraphQL query cache.
// See https://github.com/wasmerio/wasmer/pull/3983 for more
pub(super) fn invalidate_graphql_query_cache(cache_dir: &Path) -> Result<(), anyhow::Error> {
    let cache_dir = cache_dir.join("queries");
    std::fs::remove_dir_all(cache_dir)?;

    Ok(())
}

// Upload a package to a signed url.
pub(super) async fn upload(
    client: &WasmerClient,
    hash: &PackageHash,
    timeout: humantime::Duration,
    bytes: Bytes,
    pb: ProgressBar,
    proxy: Option<reqwest::Proxy>,
) -> anyhow::Result<String> {
    let hash_str = hash.to_string();
    let hash_str = hash_str.trim_start_matches("sha256:");

    let session_uri = {
        let default_timeout_secs = Some(60 * 30);
        let q = wasmer_backend_api::query::get_signed_url_for_package_upload(
            client,
            default_timeout_secs,
            Some(hash_str),
            None,
            None,
            Some(UploadMethod::R2),
        );

        match q.await? {
            Some(u) => u.url,
            None => anyhow::bail!(
                "The backend did not provide a valid signed URL to upload the package"
            ),
        }
    };

    tracing::info!("signed url is: {session_uri}");

    let client = {
        let builder = reqwest::Client::builder()
            .default_headers(reqwest::header::HeaderMap::default())
            .timeout(timeout.into());

        let builder = if let Some(proxy) = proxy {
            builder.proxy(proxy)
        } else {
            builder
        };

        builder.build().unwrap()
    };

    let total_bytes = bytes.len();
    pb.set_length(total_bytes.try_into().unwrap());
    pb.set_style(ProgressStyle::with_template("{spinner:.yellow} [{elapsed_precise}] [{bar:.white}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})")
                 .unwrap()
                 .progress_chars("█▉▊▋▌▍▎▏  ")
                 .tick_strings(&["✶", "✸", "✹", "✺", "✹", "✷", "✶"]));
    tracing::info!("webc is {total_bytes} bytes long");

    let chunk_size = 8 * 1024;

    let stream = futures::stream::unfold(0, move |offset| {
        let pb = pb.clone();
        let bytes = bytes.clone();
        async move {
            if offset >= total_bytes {
                return None;
            }

            let start = offset;

            let end = if (start + chunk_size) >= total_bytes {
                total_bytes
            } else {
                start + chunk_size
            };

            let n = end - start;
            let next_chunk = bytes.slice(start..end);
            pb.inc(n as u64);

            Some((Ok::<_, std::io::Error>(next_chunk), offset + n))
        }
    });

    let res = client
        .put(&session_uri)
        .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
        .header(reqwest::header::CONTENT_LENGTH, format!("{total_bytes}"))
        .body(Body::wrap_stream(stream));

    res.send()
        .await
        .map(|response| response.error_for_status())
        .map_err(|e| anyhow::anyhow!("error uploading package to {session_uri}: {e}"))??;

    Ok(session_uri)
}

/// Read and return a manifest given a path.
///
/// For a prebuilt package (a `.webc`, or the `.webcm` sidecar naming one) the
/// returned path is the `.webc` and the manifest is reconstructed from its
/// metadata, with the sidecar (when present) as the authoritative source of
/// the package's name and version.
///
// The difference with the `load_package_manifest` is that
// this function returns an error if no manifest is found.
pub(super) fn get_manifest(path: &Path) -> anyhow::Result<(PathBuf, Manifest)> {
    if path.is_file() {
        let extension = path.extension().and_then(|s| s.to_str());
        match extension {
            Some(Webcm::EXTENSION) => {
                let webc = Webcm::require_paired_webc(path)?;
                let manifest = get_manifest_from_webc_file(&webc, Some(path))?;
                return Ok((webc, manifest));
            }
            Some(Webcm::WEBC_EXTENSION) => {
                let sidecar = Webcm::path_for_webc(path);
                let sidecar = sidecar.is_file().then_some(sidecar);
                let manifest = get_manifest_from_webc_file(path, sidecar.as_deref())?;
                return Ok((path.to_path_buf(), manifest));
            }
            _ => {}
        }
    }

    load_package_manifest(path).and_then(|j| {
        j.ok_or_else(|| anyhow::anyhow!("No valid manifest found in path '{}'", path.display()))
    })
}

/// Load a manifest from a .webc file, with the package identity taken from
/// `webcm_path` when given. The webc is verified against the hash the sidecar
/// records, if any.
fn get_manifest_from_webc_file(path: &Path, webcm_path: Option<&Path>) -> anyhow::Result<Manifest> {
    let webcm = webcm_path
        .map(|webcm_path| -> anyhow::Result<Webcm> {
            let contents = std::fs::read_to_string(webcm_path)
                .with_context(|| format!("Failed to read '{}'", webcm_path.display()))?;
            contents
                .parse()
                .with_context(|| format!("Invalid webcm '{}'", webcm_path.display()))
        })
        .transpose()?;

    // When the sidecar records a hash, read the bytes once to both verify and
    // parse; otherwise let `from_disk` load the manifest without a full read.
    let container = match webcm.as_ref().and_then(|w| w.package.hash.as_ref()) {
        Some(expected) => {
            let bytes = std::fs::read(path)
                .with_context(|| format!("Failed to read webc file '{}'", path.display()))?;
            let actual = PackageHash::from_sha256_bytes(sha2::Sha256::digest(&bytes).into());
            expected
                .ensure_matches(&actual)
                .with_context(|| format!("for webc '{}'", path.display()))?;
            from_bytes(bytes)
        }
        None => from_disk(path),
    }
    .map_err(|e| anyhow::anyhow!("Failed to load webc file '{}': {}", path.display(), e))?;

    let mut manifest = manifest_from_webc_metadata(container.manifest())?;

    if let Some(webcm) = webcm {
        let package = manifest
            .package
            .get_or_insert_with(wasmer_config::package::Package::new_empty);
        package.name = Some(webcm.package.name);
        package.version = Some(webcm.package.version);
    }

    Ok(manifest)
}

/// Convert a webc manifest into a [`Manifest`], extracting the package metadata.
///
/// Note: only the package metadata (name, version, description, etc.) is
/// extracted; modules, commands, and filesystem mappings are not, because they
/// are already baked into the webc and are not needed to describe the package.
pub(super) fn manifest_from_webc_metadata(
    webc_manifest: &webc::metadata::Manifest,
) -> anyhow::Result<Manifest> {
    // Extract package information from the webc manifest
    let mut manifest = Manifest::new_empty();

    // Get the wapm annotation which contains package metadata
    let wapm_annotation = webc_manifest
        .wapm()
        .map_err(|e| anyhow::anyhow!("Failed to read package annotation from webc: {e}"))?;

    if let Some(wapm) = wapm_annotation {
        let mut package = wasmer_config::package::Package::new_empty();
        package.name = wapm.name;
        package.version = if let Some(v) = wapm.version {
            Some(v.parse()?)
        } else {
            None
        };
        package.description = wapm.description;
        package.license = wapm.license;
        package.homepage = wapm.homepage;
        package.repository = wapm.repository;
        package.private = wapm.private;
        package.entrypoint = webc_manifest.entrypoint.clone();

        // Only set the package if at least one field is populated
        // (Package::from_manifest strips name/version/description from WAPM annotation,
        // so these might be None even for valid packages)
        manifest.package = Some(package);
    } else {
        // No WAPM annotation found - create an empty package
        manifest.package = Some(wasmer_config::package::Package::new_empty());
    }

    // Note: We don't need to extract all the details (modules, commands, fs, etc.)
    // because those are already in the webc and we won't be rebuilding it.
    // We only need the package metadata for namespace/name/version extraction.
    // If these are not present in the webc, users can provide them via CLI flags.

    Ok(manifest)
}

pub(super) async fn login_user(
    env: &WasmerEnv,
    interactive: bool,
    msg: &str,
) -> anyhow::Result<WasmerClient> {
    if let Ok(client) = env.client() {
        return Ok(client);
    }

    let theme = dialoguer::theme::ColorfulTheme::default();

    if env.token().is_none() {
        if interactive {
            eprintln!(
                "{}: You need to be logged in to {msg}.",
                "WARN".yellow().bold()
            );

            if Confirm::with_theme(&theme)
                .with_prompt("Do you want to login now?")
                .interact()?
            {
                Login {
                    no_browser: false,
                    wasmer_dir: env.dir().to_path_buf(),
                    cache_dir: env.cache_dir().to_path_buf(),
                    token: None,
                    registry: env.registry.clone(),
                }
                .run_async()
                .await?;
            } else {
                anyhow::bail!("Stopping the flow as the user is not logged in.")
            }
        } else {
            let bin_name = self::macros::bin_name!();
            eprintln!(
                "You are not logged in. Use the `--token` flag or log in (use `{bin_name} login`) to {msg}."
            );
            anyhow::bail!("Stopping execution as the user is not logged in.")
        }
    }

    env.client()
}

/// Resolve a registry's web frontend host from its GraphQL endpoint, falling
/// back to the endpoint's own domain for custom registries.
pub(super) fn registry_web_host(client: &WasmerClient) -> String {
    let host = client.graphql_endpoint().domain().unwrap_or("wasmer.io");

    // Our special cases..
    match host {
        _ if host.contains("wasmer.wtf") => "wasmer.wtf".to_string(),
        _ if host.contains("wasmer.io") => "wasmer.io".to_string(),
        _ => host.to_string(),
    }
}

/// Build a package's web frontend URL. `version` is rendered verbatim, so pass a
/// display string like `0.1.3`, not a parsed `VersionReq`.
pub(super) fn package_web_url(
    client: &WasmerClient,
    full_name: &str,
    version: Option<&str>,
) -> String {
    let host = registry_web_host(client);
    match version {
        Some(version) => format!("https://{host}/{full_name}@{version}"),
        None => format!("https://{host}/{full_name}"),
    }
}

/// Adapter over [`package_web_url`] for a [`NamedPackageIdent`].
pub(super) fn package_web_url_for_ident(client: &WasmerClient, pkg: &NamedPackageIdent) -> String {
    // `*` when no version; an exact requirement renders as `=x.y.z`.
    let version = pkg.version_or_default().to_string().replace('=', "");
    package_web_url(client, &pkg.full_name(), Some(&version))
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Context;
    use humantime::Duration as HumanDuration;
    use indicatif::ProgressBar;
    use sha2::{Digest, Sha256};
    use url::Url;
    use wasmer_package::package::Package;

    #[tokio::test]
    #[ignore = "Requires WASMER_REGISTRY_URL/WASMER_TOKEN"]
    async fn test_upload_package_r2() -> anyhow::Result<()> {
        let registry = std::env::var("WASMER_REGISTRY_URL")
            .context("set WASMER_REGISTRY_URL to point at the registry GraphQL endpoint")?;
        let token = std::env::var("WASMER_TOKEN")
            .context("set WASMER_TOKEN for the registry under test")?;
        let client = WasmerClient::new(Url::parse(&registry)?, "wasmer-cli-upload-test")?
            .with_auth_token(token);
        let pkg_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../wasmer-test-files/legacy/coreutils-1.0.11.tar.gz");
        let package = Package::from_tarball_file(&pkg_path)?;
        let bytes = package.serialize()?;
        let hash_bytes: [u8; 32] = Sha256::digest(&bytes).into();
        let hash = PackageHash::from_sha256_bytes(hash_bytes);
        let pb = ProgressBar::hidden();

        // Upload should succeed
        let upload_url = upload(
            &client,
            &hash,
            HumanDuration::from(std::time::Duration::from_secs(300)),
            package.serialize().unwrap(),
            pb,
            None,
        )
        .await?;
        assert!(
            upload_url.starts_with("http"),
            "upload returned non-url: {upload_url}"
        );
        Ok(())
    }

    #[test]
    fn test_get_manifest_from_webc() -> anyhow::Result<()> {
        use tempfile::TempDir;
        use wasmer_package::package::Package;

        // Create a temporary directory with a test package
        let temp_dir = TempDir::new()?;
        let pkg_dir = temp_dir.path();

        // Create wasmer.toml
        std::fs::write(
            pkg_dir.join("wasmer.toml"),
            r#"
[package]
name = "test/mypackage"
version = "0.1.0"
description = "Test package for webc manifest extraction"

[fs]
data = "data"
"#,
        )?;

        // Create data directory
        std::fs::create_dir(pkg_dir.join("data"))?;
        std::fs::write(pkg_dir.join("data/test.txt"), "Hello World")?;

        // Build the package
        let pkg = Package::from_manifest(pkg_dir.join("wasmer.toml"))?;
        let webc_bytes = pkg.serialize()?;

        // Write the webc file
        let webc_path = pkg_dir.join("test.webc");
        std::fs::write(&webc_path, &webc_bytes)?;

        // Test that we can extract the manifest from the webc file
        let (path, manifest) = get_manifest(&webc_path)?;

        assert_eq!(path, webc_path);
        assert!(
            manifest.package.is_some(),
            "manifest.package should be present"
        );

        let package = manifest.package.unwrap();

        // These should be None because Package strips them
        assert_eq!(
            package.name, None,
            "Package name should be None in webc (stripped by Package::from_manifest)"
        );
        assert_eq!(
            package.version, None,
            "Package version should be None in webc (stripped by Package::from_manifest)"
        );
        assert_eq!(
            package.description, None,
            "Package description should be None in webc (stripped by Package::from_manifest)"
        );

        Ok(())
    }

    /// Build a stripped webc in `dir` and return its path and package hash.
    fn build_test_webc(dir: &Path) -> anyhow::Result<(PathBuf, PackageHash)> {
        use wasmer_package::package::Package;

        std::fs::write(
            dir.join("wasmer.toml"),
            r#"
[package]
name = "test/mypackage"
version = "0.1.0"

[fs]
data = "data"
"#,
        )?;
        std::fs::create_dir(dir.join("data"))?;
        std::fs::write(dir.join("data/test.txt"), "Hello World")?;

        let pkg = Package::from_manifest(dir.join("wasmer.toml"))?;
        let webc_bytes = pkg.serialize()?;
        let hash_bytes: [u8; 32] = Sha256::digest(&webc_bytes).into();

        let webc_path = dir.join("test.webc");
        std::fs::write(&webc_path, &webc_bytes)?;

        Ok((webc_path, PackageHash::from_sha256_bytes(hash_bytes)))
    }

    #[test]
    fn test_get_manifest_takes_identity_from_webcm() -> anyhow::Result<()> {
        let temp_dir = tempfile::TempDir::new()?;
        let (webc_path, hash) = build_test_webc(temp_dir.path())?;

        // The name intentionally differs from the wasmer.toml the webc was
        // built from: the sidecar is authoritative (and the webc is stripped
        // of its identity anyway).
        let webcm = Webcm::new(
            wasmer_config::package::NamedPackageId::try_new("acme/published", "9.9.9")?,
            Some(hash),
        );
        std::fs::write(Webcm::path_for_webc(&webc_path), webcm.to_toml()?)?;

        // Both the webc and its sidecar are accepted as the input path.
        for input in [webc_path.clone(), Webcm::path_for_webc(&webc_path)] {
            let (path, manifest) = get_manifest(&input)?;

            assert_eq!(path, webc_path, "input: {}", input.display());
            let package = manifest.package.unwrap();
            assert_eq!(package.name.as_deref(), Some("acme/published"));
            assert_eq!(
                package.version.map(|v| v.to_string()).as_deref(),
                Some("9.9.9")
            );
        }

        Ok(())
    }

    #[test]
    fn test_get_manifest_rejects_webcm_hash_mismatch() -> anyhow::Result<()> {
        let temp_dir = tempfile::TempDir::new()?;
        let (webc_path, _) = build_test_webc(temp_dir.path())?;

        let webcm = Webcm::new(
            wasmer_config::package::NamedPackageId::try_new("acme/published", "9.9.9")?,
            Some(format!("sha256:{}", "a".repeat(64)).parse()?),
        );
        std::fs::write(Webcm::path_for_webc(&webc_path), webcm.to_toml()?)?;

        let err = get_manifest(&webc_path).unwrap_err();
        assert!(format!("{err:#}").contains("hash mismatch"), "{err:#}");

        Ok(())
    }

    #[test]
    fn test_get_manifest_rejects_orphaned_webcm() -> anyhow::Result<()> {
        let temp_dir = tempfile::TempDir::new()?;
        let webcm_path = temp_dir.path().join("test.webcm");
        std::fs::write(
            &webcm_path,
            "[package]\nname = \"acme/published\"\nversion = \"9.9.9\"\n",
        )?;

        let err = get_manifest(&webcm_path).unwrap_err();
        assert!(format!("{err:#}").contains("no paired webc"), "{err:#}");

        Ok(())
    }
}
