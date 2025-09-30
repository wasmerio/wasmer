//! Provides package publishing functionality.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{self, Context as _};
use futures_util::StreamExt;
use semver::Version;
use sha2::{Digest, Sha256};
use thiserror::Error;
use toml;

use wasmer_backend_api::WasmerClient;
use wasmer_config::package::{Manifest, NamedPackageId, PackageHash, PackageIdent};
use wasmer_package::package::Package;

/// Conditions that can be waited on after publishing a package.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishWait {
    /// Do not wait for the package to be processed.
    None,
    /// Wait until the container (webc) is available.
    Container,
    /// Wait until native executables are available.
    NativeExecutables,
    /// Wait until bindings are available.
    Bindings,
    /// Wait until everything is available.
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WaitPackageState {
    container: bool,
    native_executables: bool,
    bindings: bool,
}

impl WaitPackageState {
    fn from_wait(w: PublishWait) -> Self {
        match w {
            PublishWait::None => Self {
                container: false,
                native_executables: false,
                bindings: false,
            },
            PublishWait::Container => Self {
                container: true,
                native_executables: false,
                bindings: false,
            },
            PublishWait::NativeExecutables => Self {
                container: true,
                native_executables: true,
                bindings: false,
            },
            PublishWait::Bindings => Self {
                container: true,
                native_executables: false,
                bindings: true,
            },
            PublishWait::All => Self {
                container: true,
                native_executables: true,
                bindings: true,
            },
        }
    }

    fn is_any(&self) -> bool {
        self.container || self.native_executables || self.bindings
    }
}

/// Progress events generated during the publish process.
#[derive(Debug, Clone)]
pub enum PublishProgress {
    Building,
    Uploading { uploaded: u64, total: u64 },
    Tagging,
    Waiting(PublishWait),
}

/// Options controlling the publish process.
#[derive(Debug, Clone)]
pub struct PublishOptions {
    pub namespace: Option<String>,
    pub name: Option<String>,
    pub version: Option<Version>,
    pub timeout: Duration,
    pub wait: PublishWait,
}

impl Default for PublishOptions {
    fn default() -> Self {
        Self {
            namespace: None,
            name: None,
            version: None,
            timeout: Duration::from_secs(60 * 5),
            wait: PublishWait::None,
        }
    }
}

/// Errors that may occur during package publishing.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum PackagePublishError {
    #[error("manifest not found at {0}")]
    ManifestNotFound(PathBuf),
    #[error("failed to read manifest: {0}")]
    ManifestRead(#[from] std::io::Error),
    #[error("failed to parse manifest: {0}")]
    ManifestParse(#[from] toml::de::Error),
    #[error("package build error: {0}")]
    PackageBuild(#[from] wasmer_package::package::WasmerPackageError),
    #[error("backend API error: {0}")]
    Api(#[from] anyhow::Error),
    #[error("unexpected error: {0}")]
    Other(Box<dyn std::error::Error + Send + Sync>),
}

/// Publish a package described by the manifest at `manifest_path`.
///
/// `progress` will be called with updates about the publishing process.
pub async fn publish_package_directory<F>(
    client: &WasmerClient,
    path: &Path,
    opts: PublishOptions,
    mut progress: F,
) -> Result<PackageIdent, PackagePublishError>
where
    F: FnMut(PublishProgress) + Send,
{
    progress(PublishProgress::Building);

    let manifest_path = if path.is_dir() {
        path.join("wasmer.toml")
    } else {
        path.to_path_buf()
    };

    let (manifest_str, manifest, bytes, hash) = tokio::task::spawn_blocking({
        let manifest_path = manifest_path.clone();
        move || {
            let manifest_str = std::fs::read_to_string(&manifest_path).map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    PackagePublishError::ManifestNotFound(manifest_path.clone())
                } else {
                    PackagePublishError::ManifestRead(e)
                }
            })?;
            let manifest: Manifest = toml::from_str(&manifest_str)?;

            let package = Package::from_manifest(&manifest_path)?;
            let bytes = package.serialize()?;
            let hash_bytes: [u8; 32] = Sha256::digest(&bytes).into();
            let hash = PackageHash::from_sha256_bytes(hash_bytes);

            Ok::<_, PackagePublishError>((manifest_str, manifest, bytes, hash))
        }
    })
    .await
    .map_err(|e| PackagePublishError::Other(Box::new(e)))??;

    let total = bytes.len() as u64;

    // Determine namespace/name from opts or manifest
    let (ns, name) = {
        let manifest_pkg = manifest.package.as_ref();
        let parsed = manifest_pkg.and_then(|p| p.name.as_deref());
        let manifest_ns = parsed.and_then(|n| n.split('/').next());
        let manifest_name = parsed.and_then(|n| n.split('/').nth(1));
        let namespace = opts
            .namespace
            .clone()
            .or_else(|| manifest_ns.map(|s| s.to_string()))
            .ok_or_else(|| PackagePublishError::Api(anyhow::anyhow!("namespace missing")))?;
        let name = opts
            .name
            .clone()
            .or_else(|| manifest_name.map(|s| s.to_string()));
        (namespace, name)
    };

    // Determine if push needed
    let push_needed = wasmer_backend_api::query::get_package_release(client, &hash.to_string())
        .await
        .map_err(PackagePublishError::Api)?
        .is_none();
    if push_needed {
        let hash_string = hash.to_string();
        let signed_url = wasmer_backend_api::query::get_signed_url_for_package_upload(
            client,
            Some(60 * 30),
            Some(hash_string.trim_start_matches("sha256:")),
            None,
            None,
        )
        .await
        .map_err(PackagePublishError::Api)?
        .ok_or_else(|| anyhow::anyhow!("backend did not return upload url"))?
        .url;

        // upload bytes in chunks
        let http = reqwest::Client::builder()
            .timeout(opts.timeout)
            .build()
            .map_err(|e| PackagePublishError::Api(e.into()))?;

        let total_len = bytes.len();
        let resp = http
            .post(&signed_url)
            .header(reqwest::header::CONTENT_LENGTH, "0")
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .header("x-goog-resumable", "start")
            .send()
            .await
            .map_err(|e| PackagePublishError::Api(e.into()))?
            .error_for_status()
            .map_err(|e| PackagePublishError::Api(e.into()))?;

        let session_url = resp
            .headers()
            .get(reqwest::header::LOCATION)
            .ok_or_else(|| {
                PackagePublishError::Api(anyhow::anyhow!(
                    "upload server did not provide session URL"
                ))
            })?
            .to_str()
            .map_err(|e| PackagePublishError::Api(e.into()))?
            .to_string();

        progress(PublishProgress::Uploading { uploaded: 0, total });
        let res = http
            .put(&session_url)
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .header(reqwest::header::CONTENT_LENGTH, total_len)
            .body(bytes)
            .send()
            .await
            .map_err(|e| PackagePublishError::Api(e.into()))?
            .error_for_status()
            .map_err(|e| PackagePublishError::Api(e.into()))?;
        drop(res);
        progress(PublishProgress::Uploading {
            uploaded: total,
            total,
        });

        wasmer_backend_api::query::push_package_release(
            client,
            name.as_deref(),
            &ns,
            &session_url,
            Some(manifest.package.as_ref().map_or(true, |p| p.private)),
        )
        .await
        .map_err(PackagePublishError::Api)?
        .ok_or_else(|| anyhow::anyhow!("push response empty"))?;
    }

    // Tagging
    progress(PublishProgress::Tagging);
    let package_release = wasmer_backend_api::query::get_package_release(client, &hash.to_string())
        .await
        .map_err(PackagePublishError::Api)?
        .ok_or_else(|| anyhow::anyhow!("package not found after push"))?;
    let version = opts
        .version
        .or_else(|| manifest.package.as_ref().and_then(|p| p.version.clone()))
        .ok_or_else(|| PackagePublishError::Api(anyhow::anyhow!("package version missing")))?;

    let package_name =
        name.ok_or_else(|| PackagePublishError::Api(anyhow::anyhow!("package name missing")))?;
    let id = NamedPackageId {
        full_name: format!("{ns}/{package_name}"),
        version,
    };

    let readme_contents =
        if let Some(readme_rel) = manifest.package.as_ref().and_then(|p| p.readme.as_ref()) {
            let parent = manifest_path.parent().ok_or_else(|| {
                PackagePublishError::Api(anyhow::anyhow!(
                    "manifest path '{}' has no parent directory",
                    manifest_path.display()
                ))
            })?;
            let readme_path = parent.join(readme_rel);
            Some(
                std::fs::read_to_string(&readme_path)
                    .with_context(|| format!("failed to read README at {}", readme_path.display()))
                    .map_err(PackagePublishError::Api)?,
            )
        } else {
            None
        };

    wasmer_backend_api::query::tag_package_release(
        client,
        manifest
            .package
            .as_ref()
            .and_then(|p| p.description.as_deref()),
        manifest
            .package
            .as_ref()
            .and_then(|p| p.homepage.as_deref()),
        manifest.package.as_ref().and_then(|p| p.license.as_deref()),
        manifest
            .package
            .as_ref()
            .and_then(|p| p.license_file.as_ref())
            .map(|p| p.to_string_lossy())
            .as_deref(),
        Some(&manifest_str),
        &id.full_name,
        Some(&ns),
        &package_release.id,
        Some(manifest.package.as_ref().map_or(true, |p| p.private)),
        readme_contents.as_deref(),
        manifest
            .package
            .as_ref()
            .and_then(|p| p.repository.as_deref()),
        &id.version.to_string(),
    )
    .await
    .map_err(PackagePublishError::Api)?
    .ok_or_else(|| anyhow::anyhow!("tag package failed"))?;

    if let PublishWait::None = opts.wait {
    } else {
        progress(PublishProgress::Waiting(opts.wait));
        wait_package(client, opts.wait, package_release.id.clone(), opts.timeout).await?;
    }

    Ok(PackageIdent::Named(id.into()))
}

async fn wait_package(
    client: &WasmerClient,
    to_wait: PublishWait,
    package_version_id: wasmer_backend_api::types::Id,
    timeout: Duration,
) -> Result<(), anyhow::Error> {
    if let PublishWait::None = to_wait {
        return Ok(());
    }

    let mut stream =
        wasmer_backend_api::subscription::package_version_ready(client, package_version_id.inner())
            .await?;
    let mut state = WaitPackageState::from_wait(to_wait);
    let deadline = std::time::Instant::now() + timeout;
    while state.is_any() {
        if std::time::Instant::now() > deadline {
            anyhow::bail!("timed out waiting for package version");
        }
        let data = tokio::time::timeout_at(deadline.into(), stream.next()).await?;
        let data = match data {
            Some(d) => d?,
            None => break,
        };
        if let Some(msg) = data.data {
            use wasmer_backend_api::types::PackageVersionState as S;
            match msg.package_version_ready.state {
                S::WebcGenerated => state.container = false,
                S::BindingsGenerated => state.bindings = false,
                S::NativeExesGenerated => state.native_executables = false,
            }
        }
    }
    Ok(())
}
