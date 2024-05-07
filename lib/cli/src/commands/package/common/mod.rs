use crate::{
    commands::Login,
    opts::{ApiOpts, WasmerEnv},
    utils::load_package_manifest,
};
use colored::Colorize;
use dialoguer::Confirm;
use semver::VersionReq;
use std::{collections::BTreeMap, path::PathBuf, str::FromStr};
use wasmer_api::WasmerClient;
use wasmer_config::package::{Manifest, NamedPackageIdent, PackageHash, PackageIdent};
use webc::wasmer_package::Package;

pub mod macros;
pub mod wait;

// We have PackageId and PackageIdent.. Brace yourselves, here we have their intertransmutunion,
// the PackageSpecifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PackageSpecifier {
    Hash {
        namespace: String,
        hash: PackageHash,
    },
    Named {
        namespace: String,
        name: String,
        tag: Tag,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Tag {
    Version(semver::Version), // <-- This is the reason..
    Hash(PackageHash),
}

impl Into<PackageIdent> for PackageSpecifier {
    fn into(self) -> PackageIdent {
        match self {
            PackageSpecifier::Hash { hash, .. } => PackageIdent::Hash(hash),
            PackageSpecifier::Named {
                namespace,
                name,
                tag,
            } => match tag {
                Tag::Version(v) => PackageIdent::Named(NamedPackageIdent {
                    registry: None,
                    namespace: Some(namespace),
                    name,
                    tag: Some(wasmer_config::package::Tag::VersionReq(
                        VersionReq::parse(&v.to_string()).unwrap(),
                    )),
                }),
                Tag::Hash(h) => PackageIdent::Named(NamedPackageIdent {
                    registry: None,
                    namespace: Some(namespace),
                    name,
                    tag: Some(wasmer_config::package::Tag::Named(h.to_string())),
                }),
            },
        }
    }
}

pub(super) fn into_specifier(
    manifest: &Manifest,
    hash: &PackageHash,
    namespace: String,
) -> anyhow::Result<PackageSpecifier> {
    Ok(match &manifest.package {
        None => PackageSpecifier::Hash {
            namespace,
            hash: hash.clone(),
        },
        Some(n) => match &n.name {
            Some(name) => {
                let named = NamedPackageIdent::from_str(&name)?;
                match &n.version {
                    Some(v) => PackageSpecifier::Named {
                        namespace,
                        name: named.name.clone(),
                        tag: Tag::Version(v.clone()),
                    },
                    None => PackageSpecifier::Named {
                        namespace,
                        name: named.name.clone(),
                        tag: Tag::Hash(hash.clone()),
                    },
                }
            }
            None => PackageSpecifier::Hash {
                namespace,
                hash: hash.clone(),
            },
        },
    })
}

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
pub(super) fn invalidate_graphql_query_cache(cache_dir: &PathBuf) -> Result<(), anyhow::Error> {
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
) -> anyhow::Result<String> {
    let hash_str = hash.to_string();
    let hash_str = hash_str.trim_start_matches("sha256:");

    let url = {
        let default_timeout_secs = Some(60 * 30);
        let q = wasmer_api::query::get_signed_url_for_package_upload(
            &client,
            default_timeout_secs,
            Some(&hash_str),
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

    tracing::info!("signed url is: {url}");

    let client = reqwest::Client::builder()
        .default_headers(reqwest::header::HeaderMap::default())
        .timeout(timeout.into())
        .build()
        .unwrap();

    let res = client
        .post(&url)
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
    tracing::info!("webc is {total_bytes} bytes long");

    let chunk_size = 1_048_576; // 1MB - 315s / 100MB
    let mut chunks = bytes.chunks(chunk_size);
    let mut total_bytes_sent = 0;

    let client = reqwest::Client::builder().build().unwrap();

    while let Some(chunk) = chunks.next() {
        // TODO: add upload pbar.

        let n = chunk.len();

        let start = total_bytes_sent;
        let end = start + chunk.len().saturating_sub(1);
        let content_range = format!("bytes {start}-{end}/{total_bytes}");

        let res = client
            .put(&session_uri)
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .header(reqwest::header::CONTENT_LENGTH, format!("{}", chunk.len()))
            .header("Content-Range".to_string(), content_range)
            .body(chunk.to_vec());

        res.send()
            .await
            .map(|response| response.error_for_status())
            .map_err(|e| {
                anyhow::anyhow!("cannot send request to {session_uri} (chunk {start}..{end}): {e}",)
            })??;

        total_bytes_sent += n;
    }

    Ok(url)
}

/// Read and return a manifest given a path.
///
// The difference with the `load_package_manifest` is that
// this function returns an error if no manifest is found.
pub(super) fn get_manifest(path: &PathBuf) -> anyhow::Result<(PathBuf, Manifest)> {
    load_package_manifest(&path).and_then(|j| {
        j.ok_or_else(|| anyhow::anyhow!("No valid manifest found in path '{}'", path.display()))
    })
}

pub(super) async fn login_user(
    api: &ApiOpts,
    env: &WasmerEnv,
    interactive: bool,
    msg: &str,
) -> anyhow::Result<WasmerClient> {
    if let Ok(client) = api.client() {
        return Ok(client);
    }

    let theme = dialoguer::theme::ColorfulTheme::default();

    if api.token.is_none() {
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
                    wasmer_dir: env.wasmer_dir.clone(),
                    registry: api
                        .registry
                        .clone()
                        .map(|l| wasmer_registry::wasmer_env::Registry::from(l.to_string())),
                    token: api.token.clone(),
                    cache_dir: Some(env.cache_dir.clone()),
                }
                .run_async()
                .await?;
                // self.api = ApiOpts::default();
            } else {
                anyhow::bail!("Stopping the push flow as the user is not logged in.")
            }
        } else {
            let bin_name = self::macros::bin_name!();
            eprintln!("You are not logged in. Use the `--token` flag or log in (use `{bin_name} login`) to {msg}.");
            anyhow::bail!("Stopping execution as the user is not logged in.")
        }
    }

    api.client()
}
