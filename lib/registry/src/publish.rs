use graphql_client::GraphQLQuery;
use wapm_toml::Package;
use std::path::PathBuf;
use log::info;
use console::{style, Emoji};
use indicatif::{ProgressBar, ProgressState, ProgressStyle};
use std::collections::BTreeMap;
use anyhow::anyhow;
use std::io::Read;
use std::fmt::Write;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "graphql/schema.graphql",
    query_path = "graphql/queries/publish_package_chunked.graphql",
    response_derives = "Debug"
)]
struct PublishPackageMutationChunked;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "graphql/schema.graphql",
    query_path = "graphql/queries/publish_package.graphql",
    response_derives = "Debug, Clone"
)]
struct PublishPackageMutation;

#[derive(GraphQLQuery)]
#[graphql(
    schema_path = "graphql/schema.graphql",
    query_path = "graphql/queries/get_signed_url.graphql",
    response_derives = "Debug, Clone"
)]
struct GetSignedUrl;

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
pub fn try_default_uploading(
    registry_url: &str,
    login_token: &str,
    package: &Package,
    manifest_string: &String,
    license_file: &Option<String>,
    readme: &Option<String>,
    archive_name: &String,
    archive_path: &PathBuf,
    maybe_signature_data: &SignArchiveResult,
    quiet: bool,
) -> Result<(), anyhow::Error> {
    let maybe_signature_data = match maybe_signature_data {
        SignArchiveResult::Ok {
            public_key_id,
            signature,
        } => {
            info!(
                "Package successfully signed with public key: \"{}\"!",
                &public_key_id
            );
            Some(publish_package_mutation::InputSignature {
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
        println!("{} {}Publishing...", style("[1/1]").bold().dim(), PACKAGE,);
    }

    // regular upload
    let q = PublishPackageMutation::build_query(publish_package_mutation::Variables {
        name: package.name.to_string(),
        version: package.version.to_string(),
        description: package.description.clone(),
        manifest: manifest_string.to_string(),
        license: package.license.clone(),
        license_file: license_file.to_owned(),
        readme: readme.to_owned(),
        repository: package.repository.clone(),
        homepage: package.homepage.clone(),
        file_name: Some(archive_name.clone()),
        signature: maybe_signature_data,
    });
    assert!(archive_path.exists());
    assert!(archive_path.is_file());

    let _response: publish_package_mutation::ResponseData = crate::graphql::execute_query_modifier_inner(
        registry_url, 
        login_token, 
        &q,
        None,
        |f| {f.file(archive_name.to_string(), archive_path).unwrap() }
    )?;

    if !quiet {
        println!(
            "Successfully published package `{}@{}`",
            package.name, package.version
        );
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn try_chunked_uploading(
    registry_url: &str,
    login_token: &str,
    package: &Package,
    manifest_string: &String,
    license_file: &Option<String>,
    readme: &Option<String>,
    archive_name: &String,
    archive_path: &PathBuf,
    maybe_signature_data: &SignArchiveResult,
    archived_data_size: u64,
    quiet: bool,
) -> Result<(), anyhow::Error> {
    let maybe_signature_data = match maybe_signature_data {
        SignArchiveResult::Ok {
            public_key_id,
            signature,
        } => {
            info!(
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
    });

    let _response: get_signed_url::ResponseData =
        crate::graphql::execute_query_modifier_inner(
            registry_url, 
            "",
            &get_google_signed_url, None,
            |f| f,
        )?;

    let url = _response.url.ok_or({
        let e = anyhow!(
            "could not get signed url for package {}@{}",
            package.name,
            package.version
        );
        #[cfg(feature = "telemetry")]
        sentry::integrations::anyhow::capture_anyhow(&e);
        e
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
        return Err(anyhow!(
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
        .open(&archive_path)
        .unwrap();

    let pb = ProgressBar::new(archived_data_size);
    pb.set_style(ProgressStyle::with_template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({eta})")
    .unwrap()
    .with_key("eta", |state: &ProgressState, w: &mut dyn Write| {
        write!(w, "{:.1}s", state.eta().as_secs_f64()).unwrap()
    })
    .progress_chars("#>-"));

    let chunk_size = 256 * 1024;
    let file_pointer = 0;

    loop {
        let mut chunk = Vec::with_capacity(chunk_size);
        let n = std::io::Read::by_ref(&mut file)
            .take(chunk_size as u64)
            .read_to_end(&mut chunk)?;
        if n == 0 {
            break;
        }

        let start = file_pointer;
        let end = file_pointer + chunk.len().saturating_sub(1);
        let content_range = format!("bytes {start}-{end}/{total}");

        let client = reqwest::blocking::Client::builder()
            .default_headers(reqwest::header::HeaderMap::default())
            .build()
            .unwrap();

        let res = client
            .put(&session_uri)
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .header(reqwest::header::CONTENT_LENGTH, format!("{}", chunk.len()))
            .header("Content-Range".to_string(), content_range)
            .body(chunk.to_vec());

        pb.set_position(file_pointer as u64);

        let _response = res.send().unwrap();

        if n < chunk_size {
            break;
        }
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
        crate::graphql::execute_query(registry_url, login_token, &q)?;

    println!(
        "Successfully published package `{}@{}`",
        package.name, package.version
    );

    Ok(())
}
