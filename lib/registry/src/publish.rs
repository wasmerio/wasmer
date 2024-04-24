use crate::graphql::queries::get_signed_url::GetSignedUrlUrl;

use crate::graphql::subscriptions::package_version_ready::PackageVersionState;
use crate::graphql::{
    mutations::{publish_package_mutation_chunked, PublishPackageMutationChunked},
    queries::{get_signed_url, GetSignedUrl},
};
use crate::subscriptions::subscribe_package_version_ready;
use crate::{format_graphql, WasmerConfig};
use anyhow::{Context, Result};
use console::{style, Emoji};
use futures_util::StreamExt;
use graphql_client::GraphQLQuery;
use indicatif::{ProgressBar, ProgressState, ProgressStyle};
use std::collections::BTreeMap;
use std::fmt::Write;
use std::io::Write as _;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::AsyncBufReadExt;
use tokio::sync::oneshot::Receiver;
use wasmer_config::package::{NamedPackageIdent, PackageHash, PackageIdent};

static UPLOAD: Emoji<'_, '_> = Emoji("📤", "");
static PACKAGE: Emoji<'_, '_> = Emoji("📦", "");
static FIRE: Emoji<'_, '_> = Emoji("🔥", "");

/// Different conditions that can be "awaited" when publishing a package.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublishWait {
    pub container: bool,
    pub native_executables: bool,
    pub bindings: bool,

    pub timeout: Option<Duration>,
}

impl PublishWait {
    pub fn is_any(self) -> bool {
        self.container || self.native_executables || self.bindings
    }

    pub fn new_none() -> Self {
        Self {
            container: false,
            native_executables: false,
            bindings: false,
            timeout: None,
        }
    }

    pub fn new_all() -> Self {
        Self {
            container: true,
            native_executables: true,
            bindings: true,
            timeout: None,
        }
    }

    pub fn new_container() -> Self {
        Self {
            container: true,
            native_executables: false,
            bindings: false,
            timeout: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum SignArchiveResult {
    Ok {
        public_key_id: String,
        signature: String,
    },
    NoKeyRegistered,
}

async fn wait_on(mut recv: Receiver<()>) {
    loop {
        _ = std::io::stdout().flush();
        if recv.try_recv().is_ok() {
            println!(".");
            break;
        } else {
            tokio::time::sleep(Duration::from_secs(1)).await;
            print!(".");
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn try_chunked_uploading(
    registry: Option<String>,
    token: Option<String>,
    package: &Option<wasmer_config::package::Package>,
    manifest_string: &String,
    license_file: &Option<String>,
    readme: &Option<String>,
    archive_name: &String,
    archive_path: &PathBuf,
    maybe_signature_data: &SignArchiveResult,
    archived_data_size: u64,
    quiet: bool,
    wait: PublishWait,
    timeout: Duration,
    patch_namespace: Option<String>,
) -> Result<Option<PackageIdent>, anyhow::Error> {
    let (registry, token) = initialize_registry_and_token(registry, token)?;

    let steps = if wait.is_any() { 3 } else { 2 };

    let maybe_signature_data = sign_package(maybe_signature_data);

    // fetch this before showing the `Uploading...` message
    // because there is a chance that the registry may not return a signed url.
    // This usually happens if the package version already exists in the registry.
    let signed_url = google_signed_url(&registry, &token, package, timeout)?;

    if !quiet {
        println!(
            "{} {} Uploading",
            style(format!("[1/{steps}]")).bold().dim(),
            UPLOAD
        );
    }

    upload_package(&signed_url.url, archive_path, archived_data_size, timeout).await?;

    let name = package.as_ref().map(|p| p.name.clone());

    let namespace = match patch_namespace {
        Some(n) => Some(n),
        None => package
            .as_ref()
            .map(|p| String::from(p.name.split_once('/').unwrap().0)),
    };

    let q =
        PublishPackageMutationChunked::build_query(publish_package_mutation_chunked::Variables {
            name,
            namespace,
            version: package.as_ref().map(|p| p.version.to_string()),
            description: package.as_ref().map(|p| p.description.clone()),
            manifest: manifest_string.to_string(),
            license: package.as_ref().and_then(|p| p.license.clone()),
            license_file: license_file.to_owned(),
            readme: readme.to_owned(),
            repository: package.as_ref().and_then(|p| p.repository.clone()),
            homepage: package.as_ref().and_then(|p| p.homepage.clone()),
            file_name: Some(archive_name.to_string()),
            signature: maybe_signature_data,
            signed_url: Some(signed_url.url),
            private: Some(match package {
                Some(p) => p.private,
                None => true,
            }),
            wait: Some(wait.is_any()),
        });

    tracing::debug!("{:#?}", q);

    let (send, recv) = tokio::sync::oneshot::channel();
    let mut wait_t = None;

    if !quiet {
        print!(
            "{} {} Publishing package",
            style(format!("[2/{steps}]")).bold().dim(),
            PACKAGE
        );

        _ = std::io::stdout().flush();
        wait_t = Some(tokio::spawn(wait_on(recv)))
    }

    let response: publish_package_mutation_chunked::ResponseData = {
        let registry = registry.clone();
        let token = token.clone();
        tokio::spawn(async move {
            crate::graphql::execute_query_with_timeout(&registry, &token, timeout, &q)
        })
        .await??
    };

    _ = send.send(());

    if let Some(wait_t) = wait_t {
        _ = wait_t.await;
    };

    tracing::debug!("{:#?}", response);

    if let Some(payload) = response.publish_package {
        if !payload.success {
            return Err(anyhow::anyhow!("Could not publish package"));
        } else if let Some(pkg_version) = payload.package_version {
            // Here we can assume that the package is *Some*.
            let package = package.clone().unwrap();

            if wait.is_any() {
                wait_for_package_version_to_become_ready(
                    &registry,
                    &token,
                    pkg_version.id,
                    quiet,
                    wait,
                    steps,
                )
                .await?;
            }

            let package_ident = PackageIdent::Named(NamedPackageIdent::from_str(&format!(
                "{}@{}",
                package.name, package.version
            ))?);
            eprintln!("Package published successfully");
            // println!("🚀 Successfully published package `{}`", package_ident);
            return Ok(Some(package_ident));
        } else if let Some(pkg_hash) = payload.package_webc {
            let package_ident = PackageIdent::Hash(
                PackageHash::from_str(&format!("sha256:{}", pkg_hash.webc_v3.unwrap().webc_sha256))
                    .unwrap(),
            );
            eprintln!("Package published successfully");
            // println!("🚀 Successfully published package `{}`", package_ident);
            return Ok(Some(package_ident));
        }

        unreachable!();
    } else {
        unreachable!();
    }
}

fn initialize_registry_and_token(
    registry: Option<String>,
    token: Option<String>,
) -> Result<(String, String), anyhow::Error> {
    let registry = match registry.as_ref() {
        Some(s) => format_graphql(s),
        None => {
            let wasmer_dir = WasmerConfig::get_wasmer_dir().map_err(|e| anyhow::anyhow!("{e}"))?;

            let config = WasmerConfig::from_file(&wasmer_dir);

            config
                .map_err(|e| anyhow::anyhow!("{e}"))?
                .registry
                .get_current_registry()
        }
    };

    let token = match token.as_ref() {
        Some(s) => s.to_string(),
        None => {
            let wasmer_dir = WasmerConfig::get_wasmer_dir().map_err(|e| anyhow::anyhow!("{e}"))?;

            let config = WasmerConfig::from_file(&wasmer_dir);

            config
                .map_err(|e| anyhow::anyhow!("{e}"))?
                .registry
                .get_login_token_for_registry(&registry)
                .ok_or_else(|| {
                    anyhow::anyhow!("cannot publish package: not logged into registry {registry:?}")
                })?
        }
    };

    Ok((registry, token))
}

fn sign_package(
    maybe_signature_data: &SignArchiveResult,
) -> Option<publish_package_mutation_chunked::InputSignature> {
    match maybe_signature_data {
        SignArchiveResult::Ok {
            public_key_id,
            signature,
        } => {
            log::info!(
                "Package successfully signed with public key: \"{}\"!",
                &public_key_id
            );
            Some(publish_package_mutation_chunked::InputSignature {
                public_key_key_id: public_key_id.to_string(),
                data: signature.to_string(),
            })
        }
        SignArchiveResult::NoKeyRegistered => {
            // TODO: uncomment this when we actually want users to start using it
            //warn!("Publishing package without a verifying signature. Consider registering a key pair with wasmer");
            None
        }
    }
}

fn google_signed_url(
    registry: &str,
    token: &str,
    package: &Option<wasmer_config::package::Package>,
    timeout: Duration,
) -> Result<GetSignedUrlUrl, anyhow::Error> {
    let get_google_signed_url = GetSignedUrl::build_query(get_signed_url::Variables {
        name: package.as_ref().map(|p| p.name.to_string()),
        version: package.as_ref().map(|p| p.version.to_string()),
        filename: match package {
            Some(_) => None,
            None => Some(format!("unnamed_package_{}", rand::random::<usize>())),
        },
        expires_after_seconds: Some(60 * 30),
    });

    let _response: get_signed_url::ResponseData = crate::graphql::execute_query_with_timeout(
        registry,
        token,
        timeout,
        &get_google_signed_url,
    )?;

    let url = _response.url.ok_or_else(|| match package {
        Some(pkg) => {
            anyhow::anyhow!(
                "could not get signed url for package {}@{}",
                pkg.name,
                pkg.version
            )
        }
        None => {
            anyhow::anyhow!("could not get signed url for unnamed package",)
        }
    })?;
    Ok(url)
}

async fn upload_package(
    signed_url: &str,
    archive_path: &PathBuf,
    archived_data_size: u64,
    timeout: Duration,
) -> Result<(), anyhow::Error> {
    let url = url::Url::parse(signed_url).context("cannot parse signed url")?;
    let client = reqwest::Client::builder()
        .default_headers(reqwest::header::HeaderMap::default())
        .timeout(timeout)
        .build()
        .unwrap();

    let res = client
        .post(url)
        .header(reqwest::header::CONTENT_LENGTH, "0")
        .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
        .header("x-goog-resumable", "start");

    let result = res.send().await.unwrap();

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

    let session_uri = headers.get("location").unwrap().clone();

    let total = archived_data_size;

    // archive_path
    let mut file = tokio::fs::OpenOptions::new()
        .read(true)
        .open(archive_path)
        .await
        .map_err(|e| anyhow::anyhow!("cannot open archive {}: {e}", archive_path.display()))?;

    let pb = ProgressBar::new(archived_data_size);
    pb.set_style(ProgressStyle::with_template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({eta})")
    .unwrap()
    .with_key("eta", |state: &ProgressState, w: &mut dyn Write| {
        write!(w, "{:.1}s", state.eta().as_secs_f64()).unwrap()
    })
    .progress_chars("#>-"));

    let chunk_size = 1_048_576; // 1MB - 315s / 100MB
    let mut file_pointer = 0;

    let mut reader = tokio::io::BufReader::with_capacity(chunk_size, &mut file);

    let client = reqwest::Client::builder()
        .default_headers(reqwest::header::HeaderMap::default())
        .build()
        .unwrap();

    while let Some(chunk) = reader.fill_buf().await.ok().map(|s| s.to_vec()) {
        let n = chunk.len();

        if chunk.is_empty() {
            break;
        }

        let start = file_pointer;
        let end = file_pointer + chunk.len().saturating_sub(1);
        let content_range = format!("bytes {start}-{end}/{total}");

        let res = client
            .put(&session_uri)
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .header(reqwest::header::CONTENT_LENGTH, format!("{}", chunk.len()))
            .header("Content-Range".to_string(), content_range)
            .body(chunk.to_vec());

        pb.set_position(file_pointer as u64);

        res.send()
            .await
            .map(|response| response.error_for_status())
            .map_err(|e| {
                anyhow::anyhow!(
                    "cannot send request to {session_uri} (chunk {}..{}): {e}",
                    file_pointer,
                    file_pointer + chunk_size
                )
            })??;

        if n < chunk_size {
            break;
        }

        reader.consume(n);
        file_pointer += n;
    }

    pb.finish_and_clear();
    Ok(())
}

struct PackageVersionReadySharedState {
    webc_generated: Arc<Mutex<Option<bool>>>,
    bindings_generated: Arc<Mutex<Option<bool>>>,
    native_exes_generated: Arc<Mutex<Option<bool>>>,
}

impl PackageVersionReadySharedState {
    fn new() -> Self {
        Self {
            webc_generated: Arc::new(Mutex::new(Option::None)),
            bindings_generated: Arc::new(Mutex::new(Option::None)),
            native_exes_generated: Arc::new(Mutex::new(Option::None)),
        }
    }
}

// fn create_spinner(m: &MultiProgress, message: String) -> ProgressBar {
//     let spinner = m.add(ProgressBar::new_spinner());
//     spinner.set_message(message);
//     spinner.set_style(ProgressStyle::default_spinner());
//     spinner.enable_steady_tick(Duration::from_millis(100));
//     spinner
// }
//
// fn show_spinners_while_waiting(state: &PackageVersionReadySharedState) {
//     // Clone shared state for threads
//     let (state_webc, state_bindings, state_native) = (
//         Arc::clone(&state.webc_generated),
//         Arc::clone(&state.bindings_generated),
//         Arc::clone(&state.native_exes_generated),
//     );
//     let m = MultiProgress::new();
//
//     let webc_spinner = create_spinner(&m, String::from("Generating package..."));
//     let bindings_spinner = create_spinner(&m, String::from("Generating language bindings..."));
//     let exe_spinner = create_spinner(&m, String::from("Generating native executables..."));
//
//     let check_and_finish = |spinner: ProgressBar, state: Arc<Mutex<Option<bool>>>, name: String| {
//         thread::spawn(move || loop {
//             match state.lock() {
//                 Ok(lock) => {
//                     if lock.is_some() {
//                         // spinner.finish_with_message(format!("✅ {} generation complete", name));
//                         spinner.finish_and_clear();
//                         break;
//                     }
//                 }
//                 Err(_) => {
//                     break;
//                 }
//             }
//             thread::sleep(Duration::from_millis(100));
//         });
//     };
//     check_and_finish(webc_spinner, state_webc, String::from("package"));
//     check_and_finish(
//         bindings_spinner,
//         state_bindings,
//         String::from("Language bindings"),
//     );
//     check_and_finish(
//         exe_spinner,
//         state_native,
//         String::from("Native executables"),
//     );
// }

async fn wait_for_package_version_to_become_ready(
    registry: &str,
    token: &str,
    package_version_id: impl AsRef<str>,
    quiet: bool,
    mut conditions: PublishWait,
    steps: usize,
) -> Result<()> {
    let (mut stream, _client) =
        subscribe_package_version_ready(registry, token, package_version_id.as_ref()).await?;

    let state = PackageVersionReadySharedState::new();

    let (send, recv) = tokio::sync::oneshot::channel();
    let mut wait_t = None;

    if !quiet {
        print!(
            "{} {} Waiting for package to be available",
            style(format!("[3/{steps}]")).bold().dim(),
            FIRE
        );
        _ = std::io::stdout().flush();
        wait_t = Some(tokio::spawn(wait_on(recv)));
    }

    if !conditions.is_any() {
        return Ok(());
    }

    let deadline = conditions
        .timeout
        .map(|x| std::time::Instant::now() + x)
        .unwrap_or_else(|| std::time::Instant::now() + std::time::Duration::from_secs(60 * 10));

    loop {
        if !conditions.is_any() {
            break;
        }
        if std::time::Instant::now() > deadline {
            _ = send.send(());
            return Err(anyhow::anyhow!(
                "Timed out waiting for package version to become ready"
            ));
        }

        let data = match tokio::time::timeout_at(deadline.into(), stream.next()).await {
            Err(_) => {
                _ = send.send(());
                return Err(anyhow::anyhow!(
                    "Timed out waiting for package version to become ready"
                ));
            }
            Ok(None) => {
                break;
            }
            Ok(Some(data)) => data,
        };

        if let Some(res_data) = data.unwrap().data {
            match res_data.package_version_ready.state {
                PackageVersionState::BINDINGS_GENERATED => {
                    let mut st = state.bindings_generated.lock().unwrap();
                    let is_ready = res_data.package_version_ready.success;
                    *st = Some(is_ready);
                    conditions.bindings = false;
                }
                PackageVersionState::NATIVE_EXES_GENERATED => {
                    let mut st = state.native_exes_generated.lock().unwrap();
                    *st = Some(res_data.package_version_ready.success);

                    conditions.native_executables = false;
                }
                PackageVersionState::WEBC_GENERATED => {
                    let mut st = state.webc_generated.lock().unwrap();
                    *st = Some(res_data.package_version_ready.success);

                    conditions.container = false;
                }
                PackageVersionState::Other(_) => {}
            }
        }
    }

    _ = send.send(());

    if let Some(wait_t) = wait_t {
        _ = wait_t.await;
    }

    Ok(())
}
