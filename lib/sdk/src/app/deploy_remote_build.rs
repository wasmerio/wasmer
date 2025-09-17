//! Functionality for deploying applications to Wasmer Edge through the
//! "autobuild zip upload" flow, which just uploads a zip directory and
//! lets the Wasmer backend handle building and deploying it.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{bail, Context as _};
use futures_util::StreamExt;
use reqwest::header::CONTENT_TYPE;
use walkdir::WalkDir;
use wasmer_backend_api::{
    types::{AutoBuildDeployAppLogKind, AutobuildLog, DeployAppVersion, Id},
    WasmerClient,
};
use wasmer_config::app::AppConfigV1;
use zip::{write::SimpleFileOptions, CompressionMethod};

pub use wasmer_backend_api::types::BuildConfig;

/// Options for remote deployments through [`deploy_app_remote`].
#[derive(Debug, Clone)]
pub struct DeployRemoteOpts {
    pub app: AppConfigV1,
    pub owner: Option<String>,
}

/// Events emitted during the remote deployment process.
///
/// Used by the `on_progress` callback in [`deploy_app_remote`].
#[derive(Debug, Clone)]
pub enum DeployRemoteEvent {
    /// Starting creation of the archive file.
    CreatingArchive {
        path: PathBuf,
    },
    /// Archive file has been created.
    ArchiveCreated {
        file_count: usize,
        archive_size: u64,
    },
    GeneratingUploadUrl,
    UploadArchiveStart {
        archive_size: u64,
    },
    DeterminingBuildConfiguration,
    BuildConfigDetermined {
        config: BuildConfig,
    },
    InitiatingBuild {
        vars: wasmer_backend_api::types::DeployViaAutobuildVars,
    },
    StreamingAutobuildLogs {
        build_id: String,
    },
    AutobuildLog {
        log: AutobuildLog,
    },
    Finished,
}

/// Deploy an application using the remote autobuild zip upload flow.
///
/// It will build a ZIP archive of the specified `base_dir`, upload it to Wasmer,
/// and request an autobuild deployment.
///
/// The Wasmer backend will handle building and deploying the application.
pub async fn deploy_app_remote<F>(
    client: &WasmerClient,
    opts: DeployRemoteOpts,
    base_dir: &Path,
    mut on_progress: F,
) -> Result<DeployAppVersion, anyhow::Error>
where
    F: FnMut(DeployRemoteEvent) + Send,
{
    if !base_dir.is_dir() {
        bail!(
            "deployment directory '{}' does not exist",
            base_dir.display()
        );
    }

    let app = opts.app;
    let owner = opts
        .owner
        .clone()
        .or_else(|| app.owner.clone())
        .context("owner must be specified for remote deployments")?;

    let app_name = app.name.clone();
    let app_id = app.app_id.clone();
    if app_name.is_none() && app_id.is_none() {
        bail!("remote deployments require `app.yaml` to define either an app name or an app_id");
    }

    on_progress(DeployRemoteEvent::CreatingArchive {
        path: base_dir.to_path_buf(),
    });
    let archive = create_zip_archive(base_dir)?;
    on_progress(DeployRemoteEvent::ArchiveCreated {
        file_count: archive.file_count,
        archive_size: archive.bytes.len() as u64,
    });

    let UploadArchive { bytes, .. } = archive;

    let base_for_filename = app_name.as_deref().or(app_id.as_deref()).unwrap();
    let filename = format!("{}-upload.zip", sanitize_archive_name(base_for_filename));

    on_progress(DeployRemoteEvent::GeneratingUploadUrl);

    let signed_url = wasmer_backend_api::query::generate_upload_url(
        client,
        &filename,
        app_name.as_deref(),
        None,
        Some(300),
    )
    .await?;

    on_progress(DeployRemoteEvent::UploadArchiveStart {
        archive_size: bytes.len() as u64,
    });

    let http_client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .build()
        .context("failed to create HTTP client")?;

    tracing::debug!("uploading archive to signed URL: {}", signed_url.url);
    http_client
        .put(&signed_url.url)
        .header(CONTENT_TYPE, "application/zip")
        .body(bytes)
        .send()
        .await
        .context("failed to upload archive")?
        .error_for_status()
        .context("upload rejected by storage service")?;

    let upload_url = signed_url.url;
    on_progress(DeployRemoteEvent::DeterminingBuildConfiguration);
    let config_res =
        wasmer_backend_api::query::autobuild_config_for_zip_upload(client, &upload_url)
            .await
            .context("failed to query autobuild config for uploaded archive")?
            .context("no autobuild config found for uploaded archive")?;
    let config = config_res.build_config.context(
        "Could not determine appropriate build config - project does not seem to be supported.",
    )?;
    tracing::debug!(?config, "determined build config");
    on_progress(DeployRemoteEvent::BuildConfigDetermined {
        config: config.clone(),
    });

    let app_id_value = app_id.as_ref().map(|id| Id::from(id.clone()));
    let domains: Option<Vec<Option<String>>> = app
        .domains
        .clone()
        .map(|d| d.into_iter().map(Some).collect::<Vec<_>>())
        .filter(|d| !d.is_empty());

    let vars = wasmer_backend_api::types::DeployViaAutobuildVars {
        repo_url: None,
        upload_url: Some(upload_url),
        app_name: app_name.clone(),
        app_id: app_id_value,
        owner: Some(owner),
        build_cmd: Some(String::new()),
        install_cmd: Some(String::new()),
        enable_database: Some(false),
        secrets: Some(vec![]),
        extra_data: None,
        params: None,
        managed: None,
        kind: None,
        wait_for_screenshot_generation: Some(false),
        region: None,
        branch: None,
        allow_existing_app: Some(true),
        jobs: None,
        domains,
        client_mutation_id: None,
    };
    on_progress(DeployRemoteEvent::InitiatingBuild { vars: vars.clone() });
    let deploy_response = wasmer_backend_api::query::deploy_via_autobuild(client, vars)
        .await?
        .context("deployViaAutobuild mutation did not return data")?;

    if !deploy_response.success {
        bail!("remote deployment request was rejected by the API");
    }

    let build_id = deploy_response.build_id.0;

    on_progress(DeployRemoteEvent::StreamingAutobuildLogs {
        build_id: build_id.clone(),
    });

    let mut stream =
        wasmer_backend_api::subscription::autobuild_deployment(client, &build_id).await?;

    let mut final_version: Option<DeployAppVersion> = None;

    while let Some(event) = stream.next().await {
        tracing::debug!(?event, "received autobuild event");
        let event = event?;
        if let Some(data) = event.data {
            if let Some(log) = data.autobuild_deployment {
                on_progress(DeployRemoteEvent::AutobuildLog { log: log.clone() });
                let message = log.message.clone();
                let kind = log.kind;

                match kind {
                    AutoBuildDeployAppLogKind::Failed => {
                        let msg = message.unwrap_or_else(|| "remote deployment failed".into());
                        bail!(msg);
                    }
                    AutoBuildDeployAppLogKind::Complete => {
                        let version = log.app_version.ok_or_else(|| {
                            anyhow::anyhow!(
                                "remote deployment completed but no app version was returned"
                            )
                        })?;

                        final_version = Some(version);
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    let version = final_version.ok_or_else(|| {
        anyhow::anyhow!("remote deployment stream ended without a completion event")
    })?;

    on_progress(DeployRemoteEvent::Finished);

    Ok(version)
}

struct UploadArchive {
    bytes: Vec<u8>,
    file_count: usize,
}

fn create_zip_archive(base_dir: &Path) -> Result<UploadArchive, anyhow::Error> {
    if !base_dir.join(AppConfigV1::CANONICAL_FILE_NAME).is_file() {
        bail!(
            "{} does not contain an {} file",
            base_dir.display(),
            AppConfigV1::CANONICAL_FILE_NAME
        );
    }

    let mut has_app_yaml = false;
    let mut file_count = 0usize;
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));

    let mut entries = WalkDir::new(base_dir).into_iter();
    while let Some(entry) = entries.next() {
        let entry = entry?;

        if entry.depth() == 0 {
            continue;
        }

        let rel_path = entry.path().strip_prefix(base_dir)?;
        if should_skip_archive_entry(rel_path) {
            if entry.file_type().is_dir() {
                entries.skip_current_dir();
            }
            continue;
        }

        if entry.file_type().is_symlink() {
            bail!(
                "cannot deploy projects containing symbolic links (found '{}')",
                rel_path.display()
            );
        }

        let rel_str = rel_path.to_string_lossy().replace('\\', "/");

        if entry.file_type().is_dir() {
            writer.add_directory(format!("{rel_str}/"), SimpleFileOptions::default())?;
        } else if entry.file_type().is_file() {
            if rel_path == Path::new(AppConfigV1::CANONICAL_FILE_NAME) {
                has_app_yaml = true;
            }

            file_count += 1;
            writer.start_file(
                rel_str,
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )?;
            let mut file = std::fs::File::open(entry.path())?;
            std::io::copy(&mut file, &mut writer)?;
        }
    }

    if !has_app_yaml {
        bail!("app.yaml must be included at the project root");
    }

    let cursor = writer.finish()?;
    let bytes = cursor.into_inner();

    Ok(UploadArchive { bytes, file_count })
}

fn should_skip_archive_entry(path: &Path) -> bool {
    const SKIP_DIRS: &[&str] = &[".git", ".wasmer"];

    if let Some(first) = path
        .components()
        .next()
        .and_then(|c| c.as_os_str().to_str())
    {
        if SKIP_DIRS.contains(&first) {
            return true;
        }
    } else {
        return false;
    }

    // Skip the local runtime cache shipped by the CLI.
    if path == Path::new("wasmer.app") {
        return true;
    }

    false
}

fn sanitize_archive_name(input: &str) -> String {
    let slug = input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();

    let slug = slug.trim_matches('-');

    if slug.is_empty() {
        "app".to_string()
    } else {
        slug.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, io::Cursor};
    use tempfile::tempdir;
    use zip::ZipArchive;

    #[test]
    fn create_zip_archive_ignores_wasmer_app_file() {
        let tmp = tempdir().unwrap();
        let base = tmp.path();

        // Required app configuration file.
        fs::write(
            base.join(AppConfigV1::CANONICAL_FILE_NAME),
            "name: demo\nowner: demo\n",
        )
        .unwrap();
        // Extra file that should be kept in the archive.
        fs::write(base.join("README.md"), "# demo\n").unwrap();
        // Local runtime cache file that must be ignored.
        fs::write(base.join("wasmer.app"), "{}\n").unwrap();
        // Directories that should be skipped entirely.
        fs::create_dir_all(base.join(".git/subdir")).unwrap();
        fs::write(base.join(".git/config"), "[core]\n").unwrap();
        fs::create_dir_all(base.join(".wasmer/cache")).unwrap();
        fs::write(base.join(".wasmer/cache/index"), "cache").unwrap();

        let archive = create_zip_archive(base).expect("archive generation should succeed");

        assert_eq!(archive.file_count, 2, "wasmer.app should not be counted");

        let cursor = Cursor::new(archive.bytes);
        let mut zip = ZipArchive::new(cursor).expect("zip should be readable");
        let mut entries = Vec::new();
        for idx in 0..zip.len() {
            let entry = zip.by_index(idx).expect("zip entry should open");
            entries.push(entry.name().to_string());
        }

        assert!(entries
            .iter()
            .any(|name| name == AppConfigV1::CANONICAL_FILE_NAME));
        assert!(entries.iter().any(|name| name == "README.md"));
        assert!(
            !entries.iter().any(|name| name == "wasmer.app"),
            "archive unexpectedly contains wasmer.app: {:?}",
            entries
        );
        assert!(
            entries.iter().all(|name| !name.starts_with(".git/")),
            "archive unexpectedly contains .git entries: {:?}",
            entries
        );
        assert!(
            entries.iter().all(|name| !name.starts_with(".wasmer/")),
            "archive unexpectedly contains .wasmer entries: {:?}",
            entries
        );
    }
}
