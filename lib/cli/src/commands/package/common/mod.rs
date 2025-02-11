use crate::{
    commands::{AsyncCliCommand, Login},
    config::WasmerEnv,
    utils::load_package_manifest,
};
use colored::Colorize;
use dialoguer::Confirm;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Body;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use wasmer_backend_api::WasmerClient;
use wasmer_config::package::{Manifest, NamedPackageIdent, PackageHash};
use wasmer_package::package::Package;

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
    package: &Package,
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

    let res = client
        .post(&session_uri)
        .header(reqwest::header::CONTENT_LENGTH, "0")
        .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
        .header("x-goog-resumable", "start");

    let result = res.send().await?;

    if result.status() != reqwest::StatusCode::from_u16(201).unwrap() {
        return Err(anyhow::anyhow!(
            "Uploading package failed: got HTTP {:?} when uploading",
            result.status()
        ));
    }

    let headers = result
        .headers()
        .into_iter()
        .filter_map(|(k, v)| {
            let k = k.to_string();
            let v = v.to_str().ok()?.to_string();
            Some((k.to_lowercase(), v))
        })
        .collect::<BTreeMap<_, _>>();

    let session_uri = headers
        .get("location")
        .ok_or_else(|| {
            anyhow::anyhow!("The upload server did not provide the upload URL correctly")
        })?
        .clone();

    tracing::info!("session uri is: {session_uri}");
    /* XXX: If the package is large this line may result in
     * a surge in memory use.
     *
     * In the future, we might want a way to stream bytes
     * from the webc instead of a complete in-memory
     * representation.
     */
    let bytes = package.serialize()?;

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
// The difference with the `load_package_manifest` is that
// this function returns an error if no manifest is found.
pub(super) fn get_manifest(path: &Path) -> anyhow::Result<(PathBuf, Manifest)> {
    load_package_manifest(path).and_then(|j| {
        j.ok_or_else(|| anyhow::anyhow!("No valid manifest found in path '{}'", path.display()))
    })
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
            eprintln!("You are not logged in. Use the `--token` flag or log in (use `{bin_name} login`) to {msg}.");
            anyhow::bail!("Stopping execution as the user is not logged in.")
        }
    }

    env.client()
}

pub(super) fn make_package_url(client: &WasmerClient, pkg: &NamedPackageIdent) -> String {
    let host = client.graphql_endpoint().domain().unwrap_or("wasmer.io");

    // Our special cases..
    let host = match host {
        _ if host.contains("wasmer.wtf") => "wasmer.wtf",
        _ if host.contains("wasmer.io") => "wasmer.io",
        _ => host,
    };

    format!(
        "https://{host}/{}@{}",
        pkg.full_name(),
        pkg.version_or_default().to_string().replace('=', "")
    )
}
