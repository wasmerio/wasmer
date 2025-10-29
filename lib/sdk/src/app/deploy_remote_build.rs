//! Functionality for deploying applications to Wasmer Edge through the
//! "autobuild zip upload" flow, which just uploads a zip directory and
//! lets the Wasmer backend handle building and deploying it.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context as _, bail};
use futures_util::StreamExt;
use reqwest::header::CONTENT_TYPE;
use wasmer_backend_api::{
    WasmerClient,
    types::{AutoBuildDeployAppLogKind, AutobuildLog, DeployAppVersion, Id},
};
use wasmer_config::app::AppConfigV1;
use zip::{CompressionMethod, write::SimpleFileOptions};

use thiserror::Error;

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
#[non_exhaustive]
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

/// Errors that can occur during remote deployments.
#[derive(Debug, Error)]
pub enum DeployRemoteError {
    #[error("deployment directory '{0}' does not exist")]
    MissingDeploymentDirectory(String),
    #[error("owner must be specified for remote deployments")]
    MissingOwner,
    #[error("remote deployments require `app.yaml` to define either an app name or an app_id")]
    MissingAppIdentifier,
    #[error("remote deployment request was rejected by the API")]
    RequestRejected,
    #[error("remote deployment failed: {0}")]
    DeploymentFailed(String),
    // TODO: should not use anyhow here... but backend-api crate does.
    #[error("backend API error: {0}")]
    Api(anyhow::Error),
    #[error(transparent)]
    Http(#[from] reqwest::Error),
    #[error("remote deployment completed but no app version was returned")]
    MissingAppVersion,
    #[error("remote deployment stream ended without a completion event")]
    MissingCompletionEvent,
    #[error("zip archive creation failed: {0}")]
    ZipCreation(anyhow::Error),
    #[error("unexpected error: {0}")]
    Other(Box<dyn std::error::Error + Send + Sync>),
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
) -> Result<DeployAppVersion, DeployRemoteError>
where
    F: FnMut(DeployRemoteEvent) + Send,
{
    if !base_dir.is_dir() {
        return Err(DeployRemoteError::MissingDeploymentDirectory(
            base_dir.display().to_string(),
        ));
    }

    let app = opts.app;
    let owner = opts
        .owner
        .clone()
        .or_else(|| app.owner.clone())
        .ok_or(DeployRemoteError::MissingOwner)?;

    let app_name = app.name.clone();
    let app_id = app.app_id.clone();
    if app_name.is_none() && app_id.is_none() {
        return Err(DeployRemoteError::MissingAppIdentifier);
    }

    on_progress(DeployRemoteEvent::CreatingArchive {
        path: base_dir.to_path_buf(),
    });

    let archive = tokio::task::spawn_blocking({
        let base_dir = base_dir.to_path_buf();
        move || create_zip_archive(&base_dir)
    })
    .await
    .map_err(|e| DeployRemoteError::Other(e.into()))?
    .map_err(DeployRemoteError::ZipCreation)?;
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
    .await
    .map_err(DeployRemoteError::Api)?;

    on_progress(DeployRemoteEvent::UploadArchiveStart {
        archive_size: bytes.len() as u64,
    });

    let http_client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| DeployRemoteError::Other(e.into()))?;

    tracing::debug!("uploading archive to signed URL: {}", signed_url.url);
    http_client
        .put(&signed_url.url)
        .header(CONTENT_TYPE, "application/zip")
        .body(bytes)
        .send()
        .await?
        .error_for_status()?;

    let upload_url = signed_url.url;
    on_progress(DeployRemoteEvent::DeterminingBuildConfiguration);
    let config_res =
        wasmer_backend_api::query::autobuild_config_for_zip_upload(client, &upload_url)
            .await
            .context("failed to query autobuild config for uploaded archive")
            .map_err(DeployRemoteError::Api)?
            .context("no autobuild config found for uploaded archive")
            .map_err(DeployRemoteError::Api)?;
    let config = config_res
        .build_config
        .context(
            "Could not determine appropriate build config - project does not seem to be supported.",
        )
        .map_err(DeployRemoteError::Api)?;
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
        .await
        .map_err(DeployRemoteError::Api)?
        .context("deployViaAutobuild mutation did not return data")
        .map_err(DeployRemoteError::Api)?;

    if !deploy_response.success {
        return Err(DeployRemoteError::RequestRejected);
    }

    let build_id = deploy_response.build_id.0;

    on_progress(DeployRemoteEvent::StreamingAutobuildLogs {
        build_id: build_id.clone(),
    });

    let mut final_version: Option<DeployAppVersion> = None;
    'OUTER: loop {
        let mut stream = wasmer_backend_api::subscription::autobuild_deployment(client, &build_id)
            .await
            .map_err(DeployRemoteError::Api)?;

        while let Some(event) = stream.next().await {
            tracing::debug!(?event, "received autobuild event");
            let event = event.map_err(|err| DeployRemoteError::Other(err.into()))?;
            if let Some(data) = event.data
                && let Some(log) = data.autobuild_deployment {
                    on_progress(DeployRemoteEvent::AutobuildLog { log: log.clone() });
                    let message = log.message.clone();
                    let kind = log.kind;

                    match kind {
                        AutoBuildDeployAppLogKind::Failed => {
                            let msg = message.unwrap_or_else(|| "remote deployment failed".into());
                            return Err(DeployRemoteError::DeploymentFailed(msg));
                        }
                        AutoBuildDeployAppLogKind::Complete => {
                            let version = log
                                .app_version
                                .ok_or(DeployRemoteError::MissingAppVersion)?;

                            final_version = Some(version);
                            break 'OUTER;
                        }
                        _ => {}
                    }
                }
        }

        if final_version.is_some() {
            break;
        }
        tracing::warn!("autobuild event stream ended, reconnecting...");
    }

    let version = final_version.ok_or(DeployRemoteError::MissingCompletionEvent)?;

    on_progress(DeployRemoteEvent::Finished);

    Ok(version)
}

struct UploadArchive {
    bytes: Vec<u8>,
    file_count: usize,
}

fn create_zip_archive(base_dir: &Path) -> Result<UploadArchive, anyhow::Error> {
    let mut file_count = 0usize;
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));

    let walker = {
        let mut b = ignore::WalkBuilder::new(base_dir);

        b.standard_filters(true)
            .ignore(true)
            .git_ignore(true)
            .git_exclude(true)
            .git_global(true)
            .require_git(true)
            .parents(true)
            .follow_links(false);

        // Ignore .shipit directories, since they are for local use only.
        let mut overrides = ignore::overrides::OverrideBuilder::new(".");
        overrides.add("!.shipit").expect("valid override");
        b.overrides(overrides.build()?);

        b.build()
    };

    let entries = walker.into_iter();
    for entry in entries {
        let entry = entry?;

        let ty = entry.file_type().ok_or_else(|| {
            anyhow::anyhow!(
                "failed to determine file type for '{}'",
                entry.path().display()
            )
        })?;

        let rel_path = entry.path().strip_prefix(base_dir)?;

        if ty.is_symlink() {
            bail!(
                "cannot deploy projects containing symbolic links (found '{}')",
                rel_path.display()
            );
        }

        let rel_str = rel_path.to_string_lossy().replace('\\', "/");

        if ty.is_dir() {
            writer.add_directory(format!("{rel_str}/"), SimpleFileOptions::default())?;
        } else if ty.is_file() {
            file_count += 1;
            writer.start_file(
                rel_str,
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )?;
            let mut file = std::fs::File::open(entry.path())?;
            std::io::copy(&mut file, &mut writer)?;
        }
    }

    let cursor = writer.finish()?;
    let bytes = cursor.into_inner();

    Ok(UploadArchive { bytes, file_count })
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
    use std::{collections::HashSet, fs, io::Cursor, path::Path};
    use tempfile::TempDir;

    #[test]
    fn create_zip_archive_respects_ignore_files() -> anyhow::Result<()> {
        let project = create_sample_project()?;
        let archive = create_zip_archive(project.path())?;

        let names = archive_file_names(&archive.bytes)?;

        assert!(names.contains("app.yaml"));
        assert!(names.contains("keep_dir/keep.txt"));
        // .wasmerignore should *not* be taken into account for remote builds
        assert!(names.contains("custom.txt"));
        assert!(!names.contains("ignored.txt"));
        assert!(!names.contains("ignored_dir/file.txt"));

        Ok(())
    }

    fn archive_file_names(bytes: &[u8]) -> anyhow::Result<HashSet<String>> {
        let cursor = Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(cursor)?;
        let mut names = HashSet::new();

        for idx in 0..archive.len() {
            let file = archive.by_index(idx)?;
            names.insert(file.name().to_string());
        }

        Ok(names)
    }

    fn create_sample_project() -> anyhow::Result<TempDir> {
        let dir = tempfile::tempdir()?;
        populate_project(dir.path())?;
        Ok(dir)
    }

    fn populate_project(base: &Path) -> anyhow::Result<()> {
        fs::create_dir_all(base.join(".git"))?;
        fs::write(base.join("app.yaml"), "name = \"demo\"\n")?;
        fs::write(base.join(".gitignore"), "ignored.txt\nignored_dir/\n")?;
        fs::write(base.join(".wasmerignore"), "custom.txt\n")?;
        fs::write(base.join("ignored.txt"), "ignore me")?;
        fs::write(base.join("custom.txt"), "ignore me too")?;
        fs::write(base.join("keep.txt"), "keep me")?;
        fs::create_dir_all(base.join("ignored_dir"))?;
        fs::write(base.join("ignored_dir/file.txt"), "ignored dir file")?;
        fs::create_dir_all(base.join("keep_dir"))?;
        fs::write(base.join("keep_dir/keep.txt"), "keep dir file")?;
        Ok(())
    }
}
