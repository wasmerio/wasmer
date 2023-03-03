use std::collections::BTreeMap;
use std::fmt::Write;
use std::io::BufRead;
use std::path::PathBuf;

use console::{style, Emoji};
use graphql_client::GraphQLQuery;
use indicatif::{ProgressBar, ProgressState, ProgressStyle};

use crate::graphql::{
    execute_query_modifier_inner,
    mutations::{publish_package_mutation_chunked, PublishPackageMutationChunked},
    queries::{get_signed_url, GetSignedUrl},
};
use crate::{format_graphql, WasmerConfig};

static UPLOAD: Emoji<'_, '_> = Emoji("⬆️  ", "");
static PACKAGE: Emoji<'_, '_> = Emoji("📦  ", "");

#[derive(Debug, Clone)]
pub enum SignArchiveResult {
    Ok {
        public_key_id: String,
        signature: String,
    },
    NoKeyRegistered,
}

#[allow(clippy::too_many_arguments)]
pub fn try_chunked_uploading(
    registry: Option<String>,
    token: Option<String>,
    package: &wasmer_toml::Package,
    manifest_string: &String,
    license_file: &Option<String>,
    readme: &Option<String>,
    archive_name: &String,
    archive_path: &PathBuf,
    maybe_signature_data: &SignArchiveResult,
    archived_data_size: u64,
    quiet: bool,
) -> Result<(), anyhow::Error> {
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

    let maybe_signature_data = match maybe_signature_data {
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
            //warn!("Publishing package without a verifying signature. Consider registering a key pair with wapm");
            None
        }
    };

    if !quiet {
        println!("{} {} Uploading...", style("[1/2]").bold().dim(), UPLOAD);
    }

    let get_google_signed_url = GetSignedUrl::build_query(get_signed_url::Variables {
        name: package.name.to_string(),
        version: package.version.to_string(),
        expires_after_seconds: Some(60 * 30),
    });

    let _response: get_signed_url::ResponseData =
        execute_query_modifier_inner(&registry, &token, &get_google_signed_url, None, |f| f)?;

    let url = _response.url.ok_or_else(|| {
        anyhow::anyhow!(
            "could not get signed url for package {}@{}",
            package.name,
            package.version
        )
    })?;

    let signed_url = url.url;
    let url = url::Url::parse(&signed_url).unwrap();
    let client = reqwest::blocking::Client::builder()
        .default_headers(reqwest::header::HeaderMap::default())
        .build()
        .unwrap();

    let res = client
        .post(url)
        .header(reqwest::header::CONTENT_LENGTH, "0")
        .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
        .header("x-goog-resumable", "start");

    let result = res.send().unwrap();

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
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .open(archive_path)
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

    let mut reader = std::io::BufReader::with_capacity(chunk_size, &mut file);

    let client = reqwest::blocking::Client::builder()
        .default_headers(reqwest::header::HeaderMap::default())
        .build()
        .unwrap();

    while let Some(chunk) = reader.fill_buf().ok().map(|s| s.to_vec()) {
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

    if !quiet {
        println!("{} {}Publishing...", style("[2/2]").bold().dim(), PACKAGE);
    }

    let q =
        PublishPackageMutationChunked::build_query(publish_package_mutation_chunked::Variables {
            name: package.name.to_string(),
            version: package.version.to_string(),
            description: package.description.clone(),
            manifest: manifest_string.to_string(),
            license: package.license.clone(),
            license_file: license_file.to_owned(),
            readme: readme.to_owned(),
            repository: package.repository.clone(),
            homepage: package.homepage.clone(),
            file_name: Some(archive_name.to_string()),
            signature: maybe_signature_data,
            signed_url: Some(signed_url),
        });

    let _response: publish_package_mutation_chunked::ResponseData =
        crate::graphql::execute_query(&registry, &token, &q)?;

    println!(
        "Successfully published package `{}@{}`",
        package.name, package.version
    );

    Ok(())
}
