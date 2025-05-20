use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, SystemTime},
};

use anyhow::{Context, Error};
use http::{HeaderMap, Method};
use semver::{Version, VersionReq};
use url::Url;
use wasmer_config::package::{NamedPackageId, PackageHash, PackageId, PackageIdent, PackageSource};
use webc::metadata::Manifest;

use crate::{
    http::{HttpClient, HttpRequest, USER_AGENT},
    runtime::resolver::{
        DistributionInfo, PackageInfo, PackageSummary, QueryError, Source, WebcHash,
    },
};

/// A [`Source`] which will resolve dependencies by pinging a Wasmer-like GraphQL
/// endpoint.
#[derive(Debug, Clone)]
pub struct BackendSource {
    registry_endpoint: Url,
    client: Arc<dyn HttpClient + Send + Sync>,
    cache: Option<FileSystemCache>,
    token: Option<String>,
    preferred_webc_version: webc::Version,
}

impl BackendSource {
    pub const WASMER_DEV_ENDPOINT: &'static str = "https://registry.wasmer.wtf/graphql";
    pub const WASMER_PROD_ENDPOINT: &'static str = "https://registry.wasmer.io/graphql";

    pub fn new(registry_endpoint: Url, client: Arc<dyn HttpClient + Send + Sync>) -> Self {
        BackendSource {
            registry_endpoint,
            client,
            cache: None,
            token: None,
            preferred_webc_version: webc::Version::V3,
        }
    }

    /// Cache query results locally.
    pub fn with_local_cache(self, cache_dir: impl Into<PathBuf>, timeout: Duration) -> Self {
        BackendSource {
            cache: Some(FileSystemCache::new(cache_dir, timeout)),
            ..self
        }
    }

    pub fn with_auth_token(self, token: impl Into<String>) -> Self {
        BackendSource {
            token: Some(token.into()),
            ..self
        }
    }

    pub fn with_preferred_webc_version(self, version: webc::Version) -> Self {
        BackendSource {
            preferred_webc_version: version,
            ..self
        }
    }

    pub fn registry_endpoint(&self) -> &Url {
        &self.registry_endpoint
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn query_graphql_named(&self, package_name: &str) -> Result<WebQuery, Error> {
        #[derive(serde::Serialize)]
        struct Body {
            query: String,
        }

        let body = Body {
            query: WASMER_WEBC_QUERY_ALL.replace("$NAME", package_name),
        };

        let request = HttpRequest {
            url: self.registry_endpoint.clone(),
            method: Method::POST,
            body: Some(serde_json::to_string(&body)?.into_bytes()),
            headers: self.headers(),
            options: Default::default(),
        };

        tracing::debug!(%request.url, %request.method, "Querying the GraphQL API");
        tracing::trace!(?request.headers, request.body=body.query.as_str());

        let response = self.client.request(request).await?;

        if !response.is_ok() {
            let url = &self.registry_endpoint;
            let status = response.status;

            let body = if let Some(body) = &response.body {
                String::from_utf8_lossy(body).into_owned()
            } else {
                "<no body>".to_string()
            };

            tracing::warn!(
                %url,
                %status,
                package=%package_name,
                %body,
                "failed to query package info from registry"
            );

            anyhow::bail!("\"{url}\" replied with {status}");
        }

        let body = response.body.unwrap_or_default();
        tracing::trace!(
            %response.status,
            %response.redirected,
            ?response.headers,
            "Received a response from GraphQL",
        );

        let response: WebQuery =
            serde_json::from_slice(&body).context("Unable to deserialize the response")?;

        Ok(response)
    }

    #[tracing::instrument(level = "debug", skip_all)]
    async fn query_graphql_by_hash(
        &self,
        hash: &PackageHash,
    ) -> Result<Option<PackageWebc>, Error> {
        #[derive(serde::Serialize)]
        struct Body {
            query: String,
        }

        let body = Body {
            query: WASMER_WEBC_QUERY_BY_HASH.replace("$HASH", &hash.to_string()),
        };

        let request = HttpRequest {
            url: self.registry_endpoint.clone(),
            method: Method::POST,
            body: Some(serde_json::to_string(&body)?.into_bytes()),
            headers: self.headers(),
            options: Default::default(),
        };

        tracing::debug!(%request.url, %request.method, "Querying the GraphQL API");
        tracing::trace!(?request.headers, request.body=body.query.as_str());

        let response = self.client.request(request).await?;

        if !response.is_ok() {
            let url = &self.registry_endpoint;
            let status = response.status;

            let body = if let Some(body) = &response.body {
                String::from_utf8_lossy(body).into_owned()
            } else {
                "<no body>".to_string()
            };

            tracing::warn!(
                %url,
                %status,
                %hash,
                %body,
                "failed to query package info from registry"
            );

            anyhow::bail!("\"{url}\" replied with {status}");
        }

        let body = response.body.unwrap_or_default();
        tracing::trace!(
            %response.status,
            %response.redirected,
            ?response.headers,
            "Received a response from GraphQL",
        );

        let response: Reply<GetPackageRelease> =
            serde_json::from_slice(&body).context("Unable to deserialize the response")?;

        Ok(response.data.get_package_release)
    }

    fn headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("Content-Type", "application/json".parse().unwrap());
        headers.insert("User-Agent", USER_AGENT.parse().unwrap());

        if let Some(token) = self.token.as_deref() {
            let raw_header = format!("Bearer {token}");

            match http::HeaderValue::from_str(&raw_header) {
                Ok(header) => {
                    headers.insert(http::header::AUTHORIZATION, header);
                }
                Err(e) => {
                    tracing::warn!(
                        error = &e as &dyn std::error::Error,
                        "Unable to parse the token into a header",
                    );
                }
            }
        }

        headers
    }

    async fn query_by_hash(
        &self,
        hash: &PackageHash,
    ) -> Result<Option<PackageSummary>, anyhow::Error> {
        // FIXME: implementing caching!

        let Some(data) = self.query_graphql_by_hash(hash).await? else {
            return Ok(None);
        };

        let summary = data.try_into_summary(hash.clone())?;

        Ok(Some(summary))
    }
}

#[async_trait::async_trait]
impl Source for BackendSource {
    #[tracing::instrument(level = "debug", skip_all, fields(%package))]
    async fn query(&self, package: &PackageSource) -> Result<Vec<PackageSummary>, QueryError> {
        let (package_name, version_constraint) = match package {
            PackageSource::Ident(PackageIdent::Named(n)) => (
                n.full_name(),
                n.version_opt().cloned().unwrap_or(semver::VersionReq::STAR),
            ),
            PackageSource::Ident(PackageIdent::Hash(hash)) => {
                // TODO: implement caching!
                match self.query_by_hash(hash).await {
                    Ok(Some(summary)) => return Ok(vec![summary]),
                    Ok(None) => {
                        return Err(QueryError::NoMatches {
                            query: package.clone(),
                            archived_versions: Vec::new(),
                        });
                    }
                    Err(error) => {
                        return Err(QueryError::new_other(error, package));
                    }
                }
            }
            _ => {
                return Err(QueryError::Unsupported {
                    query: package.clone(),
                })
            }
        };

        if let Some(cache) = &self.cache {
            match cache.lookup_cached_query(&package_name) {
                Ok(Some(cached)) => {
                    if let Ok(cached) = matching_package_summaries(
                        package,
                        cached,
                        &version_constraint,
                        self.preferred_webc_version,
                    ) {
                        tracing::debug!("Cache hit!");
                        return Ok(cached);
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    tracing::warn!(
                        package_name,
                        error = &*e,
                        "An unexpected error occurred while checking the local query cache",
                    );
                }
            }
        }

        let response = self
            .query_graphql_named(&package_name)
            .await
            .map_err(|error| QueryError::new_other(error, package))?;

        if let Some(cache) = &self.cache {
            if let Err(e) = cache.update(&package_name, &response) {
                tracing::warn!(
                    package_name,
                    error = &*e,
                    "An error occurred while caching the GraphQL response",
                );
            }
        }

        matching_package_summaries(
            package,
            response,
            &version_constraint,
            self.preferred_webc_version,
        )
    }
}

#[allow(clippy::result_large_err)]
fn matching_package_summaries(
    query: &PackageSource,
    response: WebQuery,
    version_constraint: &VersionReq,
    preferred_webc_version: webc::Version,
) -> Result<Vec<PackageSummary>, QueryError> {
    let mut summaries = Vec::new();

    let WebQueryGetPackage {
        namespace,
        package_name,
        versions,
        ..
    } = response
        .data
        .get_package
        .ok_or_else(|| QueryError::NotFound {
            query: query.clone(),
        })?;
    let mut archived_versions = Vec::new();

    for pkg_version in versions {
        let version = match Version::parse(&pkg_version.version) {
            Ok(v) => v,
            Err(e) => {
                tracing::debug!(
                    pkg.version = pkg_version.version.as_str(),
                    error = &e as &dyn std::error::Error,
                    "Skipping a version because it doesn't have a valid version number",
                );
                continue;
            }
        };

        if pkg_version.is_archived {
            tracing::debug!(
                pkg.version=%version,
                "Skipping an archived version",
            );
            archived_versions.push(version);
            continue;
        }

        if version_constraint.matches(&version) {
            match decode_summary(
                &namespace,
                &package_name,
                pkg_version,
                preferred_webc_version,
            ) {
                Ok(summary) => summaries.push(summary),
                Err(e) => {
                    tracing::debug!(
                        version=%version,
                        error=&*e,
                        "Skipping version because its metadata couldn't be parsed"
                    );
                }
            }
        }
    }

    if summaries.is_empty() {
        Err(QueryError::NoMatches {
            query: query.clone(),
            archived_versions,
        })
    } else {
        Ok(summaries)
    }
}

fn decode_summary(
    namespace: &str,
    package_name: &str,
    pkg_version: WebQueryGetPackageVersion,
    preferred_webc_version: webc::Version,
) -> Result<PackageSummary, Error> {
    let WebQueryGetPackageVersion {
        v2:
            WebQueryGetPackageVersionDistribution {
                pirita_sha256_hash: v2_pirita_sha256_hash,
                pirita_download_url: v2_pirita_download_url,
                webc_manifest: v2_manifest,
            },
        v3:
            WebQueryGetPackageVersionDistribution {
                pirita_sha256_hash: v3_pirita_sha256_hash,
                pirita_download_url: v3_pirita_download_url,
                webc_manifest: v3_manifest,
            },
        ..
    } = pkg_version;

    let (version, pirita_sha256_hash, pirita_download_url, manifest) =
        if preferred_webc_version == webc::Version::V3 {
            (
                webc::Version::V3,
                v3_pirita_sha256_hash,
                v3_pirita_download_url,
                v3_manifest,
            )
        } else {
            (
                webc::Version::V2,
                v2_pirita_sha256_hash,
                v2_pirita_download_url,
                v2_manifest,
            )
        };

    let id = PackageId::Named(NamedPackageId {
        full_name: format!("{namespace}/{package_name}"),
        version: pkg_version
            .version
            .parse()
            .context("could not parse package version")?,
    });

    let manifest = manifest.context("missing Manifest")?;
    let hash = pirita_sha256_hash.context("missing sha256")?;
    let webc = pirita_download_url.context("missing download URL")?;

    let manifest: Manifest = serde_json::from_slice(manifest.as_bytes())
        .context("Unable to deserialize the manifest")?;

    let webc_sha256 = WebcHash::parse_hex(&hash).context("invalid webc sha256 hash in manifest")?;

    Ok(PackageSummary {
        pkg: PackageInfo::from_manifest(id, &manifest, version)?,
        dist: DistributionInfo { webc, webc_sha256 },
    })
}

/// A local cache for package queries.
#[derive(Debug, Clone)]
struct FileSystemCache {
    cache_dir: PathBuf,
    timeout: Duration,
}

impl FileSystemCache {
    fn new(cache_dir: impl Into<PathBuf>, timeout: Duration) -> Self {
        FileSystemCache {
            cache_dir: cache_dir.into(),
            timeout,
        }
    }

    fn path(&self, package_name: &str) -> PathBuf {
        self.cache_dir.join(package_name)
    }

    fn lookup_cached_query(&self, package_name: &str) -> Result<Option<WebQuery>, Error> {
        let filename = self.path(package_name);

        let _span =
            tracing::debug_span!("lookup_cached_query", filename=%filename.display()).entered();

        tracing::trace!("Reading cached entry from disk");
        let json = match std::fs::read(&filename) {
            Ok(json) => json,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::debug!("Cache miss");
                return Ok(None);
            }
            Err(e) => {
                return Err(
                    Error::new(e).context(format!("Unable to read \"{}\"", filename.display()))
                );
            }
        };

        let entry: CacheEntry = match serde_json::from_slice(&json) {
            Ok(entry) => entry,
            Err(e) => {
                // If the entry is invalid, we should delete it to avoid work
                // in the future
                let _ = std::fs::remove_file(&filename);

                return Err(Error::new(e).context("Unable to parse the cached query"));
            }
        };

        if !entry.is_still_valid(self.timeout) {
            tracing::debug!(timestamp = entry.unix_timestamp, "Cached entry is stale");
            let _ = std::fs::remove_file(&filename);
            return Ok(None);
        }

        if entry.package_name != package_name {
            let _ = std::fs::remove_file(&filename);
            anyhow::bail!(
                "The cached response at \"{}\" corresponds to the \"{}\" package, but expected \"{}\"",
                filename.display(),
                entry.package_name,
                package_name,
            );
        }

        Ok(Some(entry.response))
    }

    fn update(&self, package_name: &str, response: &WebQuery) -> Result<(), Error> {
        let entry = CacheEntry {
            unix_timestamp: SystemTime::UNIX_EPOCH
                .elapsed()
                .unwrap_or_default()
                .as_secs(),
            package_name: package_name.to_string(),
            response: response.clone(),
        };

        let _ = std::fs::create_dir_all(&self.cache_dir);

        // First, save our cache entry to disk
        let mut temp = tempfile::NamedTempFile::new_in(&self.cache_dir)
            .context("Unable to create a temporary file")?;
        serde_json::to_writer_pretty(&mut temp, &entry)
            .context("Unable to serialize the cache entry")?;
        temp.as_file()
            .sync_all()
            .context("Flushing the temp file failed")?;

        // Now we've saved our cache entry we need to move it to the right
        // location. We do this in two steps so concurrent queries don't see
        // the cache entry until it has been completely written.
        let filename = self.path(package_name);
        tracing::debug!(
            filename=%filename.display(),
            package_name,
            "Saving the query to disk",
        );

        if let Some(parent) = filename.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        temp.persist(&filename).with_context(|| {
            format!(
                "Unable to persist the temp file to \"{}\"",
                filename.display()
            )
        })?;

        Ok(())
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct CacheEntry {
    unix_timestamp: u64,
    package_name: String,
    response: WebQuery,
}

/// Cache entry for a webc lookup by hash.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct HashCacheEntry {
    unix_timestamp: u64,
    hash: String,
    response: WebQuery,
}

impl CacheEntry {
    fn is_still_valid(&self, timeout: Duration) -> bool {
        let timestamp = SystemTime::UNIX_EPOCH + Duration::from_secs(self.unix_timestamp);

        match timestamp.elapsed() {
            Ok(duration) if duration <= timeout => true,
            Ok(_) => {
                // The cached response is too old
                false
            }
            Err(_) => {
                // It looks like the current time is **after** the time this
                // entry was recorded. That probably indicates a clock issue
                // so we should mark the cached value as invalid.
                false
            }
        }
    }
}

#[allow(dead_code)]
pub const WASMER_WEBC_QUERY_ALL: &str = r#"{
    getPackage(name: "$NAME") {
        packageName
        namespace
        versions {
          version
          isArchived
          v2: distribution(version: V2) {
            piritaDownloadUrl
            piritaSha256Hash
            webcManifest
          }
          v3: distribution(version: V3) {
            piritaDownloadUrl
            piritaSha256Hash
            webcManifest
          }
        }
    }
    info {
        defaultFrontend
    }
}"#;

pub const WASMER_WEBC_QUERY_BY_HASH: &str = r#"{
    getPackageRelease(hash: "$HASH") {
        piritaManifest
        isArchived
        webcUrl
    }
}"#;

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Reply<T> {
    pub data: T,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
struct GetPackageRelease {
    #[serde(rename = "getPackageRelease")]
    get_package_release: Option<PackageWebc>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
struct PackageWebc {
    #[serde(rename = "piritaManifest")]
    pub pirita_manifest: String,
    #[serde(rename = "isArchived")]
    pub is_archived: bool,
    #[serde(rename = "webcUrl")]
    pub webc_url: url::Url,
}

impl PackageWebc {
    fn try_into_summary(self, hash: PackageHash) -> Result<PackageSummary, anyhow::Error> {
        let manifest: Manifest = serde_json::from_str(&self.pirita_manifest)
            .context("Unable to deserialize the manifest")?;

        let id = PackageId::Hash(hash.clone());

        let info = PackageInfo::from_manifest(id, &manifest, webc::Version::V3)
            .context("could not convert the manifest ")?;

        Ok(PackageSummary {
            pkg: info,
            dist: DistributionInfo {
                webc: self.webc_url,
                // TODO: replace with different hash type?
                webc_sha256: WebcHash(hash.as_sha256().context("invalid hash")?.0),
            },
        })
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct WebQuery {
    #[serde(rename = "data")]
    pub data: WebQueryData,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct WebQueryData {
    #[serde(rename = "getPackage")]
    pub get_package: Option<WebQueryGetPackage>,
    pub info: WebQueryInfo,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct WebQueryInfo {
    #[serde(rename = "defaultFrontend")]
    pub default_frontend: Url,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct WebQueryGetPackage {
    #[serde(rename = "packageName")]
    pub package_name: String,
    pub namespace: String,
    pub versions: Vec<WebQueryGetPackageVersion>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct WebQueryGetPackageVersion {
    pub version: String,
    /// Has the package been archived?
    #[serde(rename = "isArchived", default)]
    pub is_archived: bool,
    pub v2: WebQueryGetPackageVersionDistribution,
    pub v3: WebQueryGetPackageVersionDistribution,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub enum WebCVersion {
    V2,
    V3,
}

impl Default for WebCVersion {
    fn default() -> Self {
        Self::V2
    }
}

impl From<WebCVersion> for webc::Version {
    fn from(val: WebCVersion) -> Self {
        match val {
            WebCVersion::V2 => webc::Version::V2,
            WebCVersion::V3 => webc::Version::V3,
        }
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct WebQueryGetPackageVersionDistribution {
    #[serde(rename = "piritaDownloadUrl")]
    pub pirita_download_url: Option<Url>,
    #[serde(rename = "piritaSha256Hash")]
    pub pirita_sha256_hash: Option<String>,
    #[serde(rename = "webcManifest")]
    pub webc_manifest: Option<String>,
}

#[cfg(test)]
mod tests {
    use std::{str::FromStr, sync::Mutex};

    use http::{HeaderMap, StatusCode};

    use crate::{
        http::HttpResponse,
        runtime::resolver::inputs::{DistributionInfo, PackageInfo},
    };

    use super::*;

    // You can check the response with:
    // curl https://registry.wasmer.io/graphql \
    //      -H "Content-Type: application/json" \
    //      -X POST \
    //      -d '@wasmer_pack_cli_request.json' > wasmer_pack_cli_response.json
    const WASMER_PACK_CLI_REQUEST: &[u8] = br#"
    {
        "query":"{\n    getPackage(name: \"wasmer/wasmer-pack-cli\") {\n        packageName\n        namespace\n        versions {\n          version\n          isArchived\n          v2: distribution(version: V2) {\n            piritaDownloadUrl\n            piritaSha256Hash\n            webcManifest\n          }\n          v3: distribution(version: V3) {\n            piritaDownloadUrl\n            piritaSha256Hash\n            webcManifest\n          }\n        }\n    }\n    info {\n        defaultFrontend\n    }\n}"
    }
    "#;
    const WASMER_PACK_CLI_RESPONSE: &[u8] = br#"
    {
        "data": {
          "getPackage": {
            "packageName": "wasmer-pack-cli",
            "namespace": "wasmer",
            "versions": [
              {
                "version": "0.7.1",
                "isArchived": false,
                "v2": {
                    "webcManifest": "{\"atoms\": {\"wasmer-pack\": {\"kind\": \"https://webc.org/kind/wasm\", \"signature\": \"sha256:gGeLZqPitpg893Jj/nvGa+1235RezSWA9FjssopzOZY=\"}}, \"package\": {\"wapm\": {\"name\": \"wasmer/wasmer-pack-cli\", \"readme\": {\"path\": \"README.md\", \"volume\": \"metadata\"}, \"license\": \"MIT\", \"version\": \"0.7.1\", \"homepage\": \"https://wasmer.io/\", \"repository\": \"https://github.com/wasmerio/wasmer-pack\", \"description\": \"A code generator that lets you treat WebAssembly modules like native dependencies.\"}}, \"commands\": {\"wasmer-pack\": {\"runner\": \"https://webc.org/runner/wasi/command@unstable_\", \"annotations\": {\"wasi\": {\"atom\": \"wasmer-pack\", \"package\": \"wasmer/wasmer-pack-cli\", \"main_args\": null}}}}, \"entrypoint\": \"wasmer-pack\"}",
                  "piritaDownloadUrl": "https://storage.googleapis.com/wapm-registry-prod/webc/wasmer/wasmer-pack-cli/0.7.1/wasmer-pack-cli-0.7.1.webc",
                  "piritaSha256Hash": "e821047f446dd20fb6b43a1648fe98b882276dfc480f020df6f00a49f69771fa"
                },
                "v3": {
                    "webcManifest": "{\"atoms\": {\"wasmer-pack\": {\"kind\": \"https://webc.org/kind/wasm\", \"signature\": \"sha256:gGeLZqPitpg893Jj/nvGa+1235RezSWA9FjssopzOZY=\"}}, \"package\": {\"wapm\": {\"name\": \"wasmer/wasmer-pack-cli\", \"readme\": {\"path\": \"README.md\", \"volume\": \"metadata\"}, \"license\": \"MIT\", \"version\": \"0.7.1\", \"homepage\": \"https://wasmer.io/\", \"repository\": \"https://github.com/wasmerio/wasmer-pack\", \"description\": \"A code generator that lets you treat WebAssembly modules like native dependencies.\"}}, \"commands\": {\"wasmer-pack\": {\"runner\": \"https://webc.org/runner/wasi/command@unstable_\", \"annotations\": {\"wasi\": {\"atom\": \"wasmer-pack\", \"package\": \"wasmer/wasmer-pack-cli\", \"main_args\": null}}}}, \"entrypoint\": \"wasmer-pack\"}",
                  "piritaDownloadUrl": "https://storage.googleapis.com/wapm-registry-prod/webc/wasmer/wasmer-pack-cli/0.7.1/wasmer-pack-cli-0.7.1.webc",
                  "piritaSha256Hash": "e821047f446dd20fb6b43a1648fe98b882276dfc480f020df6f00a49f69771fa"
                }
              },
              {
                "version": "0.7.0",
                "isArchived": false,
                "v2": {
                    "webcManifest": "{\"atoms\": {\"wasmer-pack\": {\"kind\": \"https://webc.org/kind/wasm\", \"signature\": \"sha256:FesCIAS6URjrIAAyy4G5u5HjJjGQBLGmnafjHPHRvqo=\"}}, \"package\": {\"wapm\": {\"name\": \"wasmer/wasmer-pack-cli\", \"readme\": {\"path\": \"/home/consulting/Documents/wasmer/wasmer-pack/crates/cli/../../README.md\", \"volume\": \"metadata\"}, \"license\": \"MIT\", \"version\": \"0.7.0\", \"homepage\": \"https://wasmer.io/\", \"repository\": \"https://github.com/wasmerio/wasmer-pack\", \"description\": \"A code generator that lets you treat WebAssembly modules like native dependencies.\"}}, \"commands\": {\"wasmer-pack\": {\"runner\": \"https://webc.org/runner/wasi/command@unstable_\", \"annotations\": {\"wasi\": {\"atom\": \"wasmer-pack\", \"package\": \"wasmer/wasmer-pack-cli\", \"main_args\": null}}}}, \"entrypoint\": \"wasmer-pack\"}",
                    "piritaDownloadUrl": "https://storage.googleapis.com/wapm-registry-prod/webc/wasmer/wasmer-pack-cli/0.7.0/wasmer-pack-cli-0.7.0.webc",
                    "piritaSha256Hash": "d085869201aa602673f70abbd5e14e5a6936216fa93314c5b103cda3da56e29e"
                },
                "v3": {
                    "webcManifest": "{\"atoms\": {\"wasmer-pack\": {\"kind\": \"https://webc.org/kind/wasm\", \"signature\": \"sha256:FesCIAS6URjrIAAyy4G5u5HjJjGQBLGmnafjHPHRvqo=\"}}, \"package\": {\"wapm\": {\"name\": \"wasmer/wasmer-pack-cli\", \"readme\": {\"path\": \"/home/consulting/Documents/wasmer/wasmer-pack/crates/cli/../../README.md\", \"volume\": \"metadata\"}, \"license\": \"MIT\", \"version\": \"0.7.0\", \"homepage\": \"https://wasmer.io/\", \"repository\": \"https://github.com/wasmerio/wasmer-pack\", \"description\": \"A code generator that lets you treat WebAssembly modules like native dependencies.\"}}, \"commands\": {\"wasmer-pack\": {\"runner\": \"https://webc.org/runner/wasi/command@unstable_\", \"annotations\": {\"wasi\": {\"atom\": \"wasmer-pack\", \"package\": \"wasmer/wasmer-pack-cli\", \"main_args\": null}}}}, \"entrypoint\": \"wasmer-pack\"}",
                    "piritaDownloadUrl": "https://storage.googleapis.com/wapm-registry-prod/webc/wasmer/wasmer-pack-cli/0.7.0/wasmer-pack-cli-0.7.0.webc",
                    "piritaSha256Hash": "d085869201aa602673f70abbd5e14e5a6936216fa93314c5b103cda3da56e29e"
                }
              },
              {
                "version": "0.6.0",
                "isArchived": false,
                "v2": {
                    "webcManifest": "{\"atoms\": {\"wasmer-pack\": {\"kind\": \"https://webc.org/kind/wasm\", \"signature\": \"sha256:CzzhNaav3gjBkCJECGbk7e+qAKurWbcIAzQvEqsr2Co=\"}}, \"package\": {\"wapm\": {\"name\": \"wasmer/wasmer-pack-cli\", \"readme\": {\"path\": \"/home/consulting/Documents/wasmer/wasmer-pack/crates/cli/../../README.md\", \"volume\": \"metadata\"}, \"license\": \"MIT\", \"version\": \"0.6.0\", \"homepage\": \"https://wasmer.io/\", \"repository\": \"https://github.com/wasmerio/wasmer-pack\", \"description\": \"A code generator that lets you treat WebAssembly modules like native dependencies.\"}}, \"commands\": {\"wasmer-pack\": {\"runner\": \"https://webc.org/runner/wasi/command@unstable_\", \"annotations\": {\"wasi\": {\"atom\": \"wasmer-pack\", \"package\": \"wasmer/wasmer-pack-cli\", \"main_args\": null}}}}, \"entrypoint\": \"wasmer-pack\"}",
                    "piritaDownloadUrl": "https://storage.googleapis.com/wapm-registry-prod/webc/wasmer/wasmer-pack-cli/0.6.0/wasmer-pack-cli-0.6.0.webc",
                    "piritaSha256Hash": "7e1add1640d0037ff6a726cd7e14ea36159ec2db8cb6debd0e42fa2739bea52b"
                },
                "v3": {
                    "webcManifest": "{\"atoms\": {\"wasmer-pack\": {\"kind\": \"https://webc.org/kind/wasm\", \"signature\": \"sha256:CzzhNaav3gjBkCJECGbk7e+qAKurWbcIAzQvEqsr2Co=\"}}, \"package\": {\"wapm\": {\"name\": \"wasmer/wasmer-pack-cli\", \"readme\": {\"path\": \"/home/consulting/Documents/wasmer/wasmer-pack/crates/cli/../../README.md\", \"volume\": \"metadata\"}, \"license\": \"MIT\", \"version\": \"0.6.0\", \"homepage\": \"https://wasmer.io/\", \"repository\": \"https://github.com/wasmerio/wasmer-pack\", \"description\": \"A code generator that lets you treat WebAssembly modules like native dependencies.\"}}, \"commands\": {\"wasmer-pack\": {\"runner\": \"https://webc.org/runner/wasi/command@unstable_\", \"annotations\": {\"wasi\": {\"atom\": \"wasmer-pack\", \"package\": \"wasmer/wasmer-pack-cli\", \"main_args\": null}}}}, \"entrypoint\": \"wasmer-pack\"}",
                    "piritaDownloadUrl": "https://storage.googleapis.com/wapm-registry-prod/webc/wasmer/wasmer-pack-cli/0.6.0/wasmer-pack-cli-0.6.0.webc",
                    "piritaSha256Hash": "7e1add1640d0037ff6a726cd7e14ea36159ec2db8cb6debd0e42fa2739bea52b"
                }
              },
              {
                "version": "0.5.3",
                "isArchived": false,
                "v2": {
                    "webcManifest": "{\"atoms\": {\"wasmer-pack\": {\"kind\": \"https://webc.org/kind/wasm\", \"signature\": \"sha256:qdiJVfpi4icJXdR7Y5US/pJ4PjqbAq9PkU+obMZIMlE=\"}}, \"package\": {\"wapm\": {\"name\": \"wasmer/wasmer-pack-cli\", \"readme\": {\"path\": \"/home/runner/work/wasmer-pack/wasmer-pack/crates/cli/../../README.md\", \"volume\": \"metadata\"}, \"license\": \"MIT\", \"version\": \"0.5.3\", \"homepage\": \"https://wasmer.io/\", \"repository\": \"https://github.com/wasmerio/wasmer-pack\", \"description\": \"A code generator that lets you treat WebAssembly modules like native dependencies.\"}}, \"commands\": {\"wasmer-pack\": {\"runner\": \"https://webc.org/runner/wasi/command@unstable_\", \"annotations\": {\"wasi\": {\"atom\": \"wasmer-pack\", \"package\": \"wasmer/wasmer-pack-cli\", \"main_args\": null}}}}, \"entrypoint\": \"wasmer-pack\"}",
                    "piritaDownloadUrl": "https://storage.googleapis.com/wapm-registry-prod/webc/wasmer/wasmer-pack-cli/0.5.3/wasmer-pack-cli-0.5.3.webc",
                    "piritaSha256Hash": "44fdcdde23d34175887243d7c375e4e4a7e6e2cd1ae063ebffbede4d1f68f14a"
                },
                "v3": {
                    "webcManifest": "{\"atoms\": {\"wasmer-pack\": {\"kind\": \"https://webc.org/kind/wasm\", \"signature\": \"sha256:qdiJVfpi4icJXdR7Y5US/pJ4PjqbAq9PkU+obMZIMlE=\"}}, \"package\": {\"wapm\": {\"name\": \"wasmer/wasmer-pack-cli\", \"readme\": {\"path\": \"/home/runner/work/wasmer-pack/wasmer-pack/crates/cli/../../README.md\", \"volume\": \"metadata\"}, \"license\": \"MIT\", \"version\": \"0.5.3\", \"homepage\": \"https://wasmer.io/\", \"repository\": \"https://github.com/wasmerio/wasmer-pack\", \"description\": \"A code generator that lets you treat WebAssembly modules like native dependencies.\"}}, \"commands\": {\"wasmer-pack\": {\"runner\": \"https://webc.org/runner/wasi/command@unstable_\", \"annotations\": {\"wasi\": {\"atom\": \"wasmer-pack\", \"package\": \"wasmer/wasmer-pack-cli\", \"main_args\": null}}}}, \"entrypoint\": \"wasmer-pack\"}",
                    "piritaDownloadUrl": "https://storage.googleapis.com/wapm-registry-prod/webc/wasmer/wasmer-pack-cli/0.5.3/wasmer-pack-cli-0.5.3.webc",
                    "piritaSha256Hash": "44fdcdde23d34175887243d7c375e4e4a7e6e2cd1ae063ebffbede4d1f68f14a"
                }
              },
              {
                "version": "0.5.2",
                "isArchived": false,
                "v2": {
                    "webcManifest": "{\"atoms\": {\"wasmer-pack\": {\"kind\": \"https://webc.org/kind/wasm\", \"signature\": \"sha256:xiwrUFAo+cU1xW/IE6MVseiyjNGHtXooRlkYKiOKzQc=\"}}, \"package\": {\"wapm\": {\"name\": \"wasmer/wasmer-pack-cli\", \"readme\": {\"path\": \"/home/consulting/Documents/wasmer/wasmer-pack/crates/cli/../../README.md\", \"volume\": \"metadata\"}, \"license\": \"MIT\", \"version\": \"0.5.2\", \"homepage\": \"https://wasmer.io/\", \"repository\": \"https://github.com/wasmerio/wasmer-pack\", \"description\": \"A code generator that lets you treat WebAssembly modules like native dependencies.\"}}, \"commands\": {\"wasmer-pack\": {\"runner\": \"https://webc.org/runner/wasi/command@unstable_\", \"annotations\": {\"wasi\": {\"atom\": \"wasmer-pack\", \"package\": \"wasmer/wasmer-pack-cli\", \"main_args\": null}}}}, \"entrypoint\": \"wasmer-pack\"}",
                    "piritaDownloadUrl": "https://storage.googleapis.com/wapm-registry-prod/webc/wasmer/wasmer-pack-cli/0.5.2/wasmer-pack-cli-0.5.2.webc",
                    "piritaSha256Hash": "d1dbc8168c3a2491a7158017a9c88df9e0c15bed88ebcd6d9d756e4b03adde95"
                },
                "v3": {
                    "webcManifest": "{\"atoms\": {\"wasmer-pack\": {\"kind\": \"https://webc.org/kind/wasm\", \"signature\": \"sha256:xiwrUFAo+cU1xW/IE6MVseiyjNGHtXooRlkYKiOKzQc=\"}}, \"package\": {\"wapm\": {\"name\": \"wasmer/wasmer-pack-cli\", \"readme\": {\"path\": \"/home/consulting/Documents/wasmer/wasmer-pack/crates/cli/../../README.md\", \"volume\": \"metadata\"}, \"license\": \"MIT\", \"version\": \"0.5.2\", \"homepage\": \"https://wasmer.io/\", \"repository\": \"https://github.com/wasmerio/wasmer-pack\", \"description\": \"A code generator that lets you treat WebAssembly modules like native dependencies.\"}}, \"commands\": {\"wasmer-pack\": {\"runner\": \"https://webc.org/runner/wasi/command@unstable_\", \"annotations\": {\"wasi\": {\"atom\": \"wasmer-pack\", \"package\": \"wasmer/wasmer-pack-cli\", \"main_args\": null}}}}, \"entrypoint\": \"wasmer-pack\"}",
                    "piritaDownloadUrl": "https://storage.googleapis.com/wapm-registry-prod/webc/wasmer/wasmer-pack-cli/0.5.2/wasmer-pack-cli-0.5.2.webc",
                    "piritaSha256Hash": "d1dbc8168c3a2491a7158017a9c88df9e0c15bed88ebcd6d9d756e4b03adde95"
                }
              },
              {
                "version": "0.5.1",
                "isArchived": false,
                "v2": {
                    "webcManifest": "{\"atoms\": {\"wasmer-pack\": {\"kind\": \"https://webc.org/kind/wasm\", \"signature\": \"sha256:TliPwutfkFvRite/3/k3OpLqvV0EBKGwyp3L5UjCuEI=\"}}, \"package\": {\"wapm\": {\"name\": \"wasmer/wasmer-pack-cli\", \"readme\": {\"path\": \"/home/runner/work/wasmer-pack/wasmer-pack/crates/cli/../../README.md\", \"volume\": \"metadata\"}, \"license\": \"MIT\", \"version\": \"0.5.1\", \"homepage\": \"https://wasmer.io/\", \"repository\": \"https://github.com/wasmerio/wasmer-pack\", \"description\": \"A code generator that lets you treat WebAssembly modules like native dependencies.\"}}, \"commands\": {\"wasmer-pack\": {\"runner\": \"https://webc.org/runner/wasi/command@unstable_\", \"annotations\": {\"wasi\": {\"atom\": \"wasmer-pack\", \"package\": \"wasmer/wasmer-pack-cli\", \"main_args\": null}}}}, \"entrypoint\": \"wasmer-pack\"}",
                    "piritaDownloadUrl": "https://storage.googleapis.com/wapm-registry-prod/webc/wasmer/wasmer-pack-cli/0.5.1/wasmer-pack-cli-0.5.1.webc",
                    "piritaSha256Hash": "c42924619660e2befd69b5c72729388985dcdcbf912d51a00015237fec3e1ade"
                },
                "v3": {
                    "webcManifest": "{\"atoms\": {\"wasmer-pack\": {\"kind\": \"https://webc.org/kind/wasm\", \"signature\": \"sha256:TliPwutfkFvRite/3/k3OpLqvV0EBKGwyp3L5UjCuEI=\"}}, \"package\": {\"wapm\": {\"name\": \"wasmer/wasmer-pack-cli\", \"readme\": {\"path\": \"/home/runner/work/wasmer-pack/wasmer-pack/crates/cli/../../README.md\", \"volume\": \"metadata\"}, \"license\": \"MIT\", \"version\": \"0.5.1\", \"homepage\": \"https://wasmer.io/\", \"repository\": \"https://github.com/wasmerio/wasmer-pack\", \"description\": \"A code generator that lets you treat WebAssembly modules like native dependencies.\"}}, \"commands\": {\"wasmer-pack\": {\"runner\": \"https://webc.org/runner/wasi/command@unstable_\", \"annotations\": {\"wasi\": {\"atom\": \"wasmer-pack\", \"package\": \"wasmer/wasmer-pack-cli\", \"main_args\": null}}}}, \"entrypoint\": \"wasmer-pack\"}",
                    "piritaDownloadUrl": "https://storage.googleapis.com/wapm-registry-prod/webc/wasmer/wasmer-pack-cli/0.5.1/wasmer-pack-cli-0.5.1.webc",
                    "piritaSha256Hash": "c42924619660e2befd69b5c72729388985dcdcbf912d51a00015237fec3e1ade"
                }
              },
              {
                "version": "0.5.0",
                "isArchived": false,
                "v2": {
                    "webcManifest": "{\"atoms\": {\"wasmer-pack\": {\"kind\": \"https://webc.org/kind/wasm\", \"signature\": \"sha256:6UD7NS4KtyNYa3TcnKOvd+kd3LxBCw+JQ8UWRpMXeC0=\"}}, \"package\": {\"wapm\": {\"name\": \"wasmer/wasmer-pack-cli\", \"readme\": {\"path\": \"README.md\", \"volume\": \"metadata\"}, \"license\": \"MIT\", \"version\": \"0.5.0\", \"homepage\": \"https://wasmer.io/\", \"repository\": \"https://github.com/wasmerio/wasmer-pack\", \"description\": \"A code generator that lets you treat WebAssembly modules like native dependencies.\"}}, \"commands\": {\"wasmer-pack\": {\"runner\": \"https://webc.org/runner/wasi/command@unstable_\", \"annotations\": {\"wasi\": {\"atom\": \"wasmer-pack\", \"package\": \"wasmer/wasmer-pack-cli\", \"main_args\": null}}}}, \"entrypoint\": \"wasmer-pack\"}",
                    "piritaDownloadUrl": "https://storage.googleapis.com/wapm-registry-prod/webc/wasmer/wasmer-pack-cli/0.5.0/wasmer-pack-cli-0.5.0.webc",
                    "piritaSha256Hash": "d30ca468372faa96469163d2d1546dd34be9505c680677e6ab86a528a268e5f5"
                },
                "v3": {
                    "webcManifest": "{\"atoms\": {\"wasmer-pack\": {\"kind\": \"https://webc.org/kind/wasm\", \"signature\": \"sha256:6UD7NS4KtyNYa3TcnKOvd+kd3LxBCw+JQ8UWRpMXeC0=\"}}, \"package\": {\"wapm\": {\"name\": \"wasmer/wasmer-pack-cli\", \"readme\": {\"path\": \"README.md\", \"volume\": \"metadata\"}, \"license\": \"MIT\", \"version\": \"0.5.0\", \"homepage\": \"https://wasmer.io/\", \"repository\": \"https://github.com/wasmerio/wasmer-pack\", \"description\": \"A code generator that lets you treat WebAssembly modules like native dependencies.\"}}, \"commands\": {\"wasmer-pack\": {\"runner\": \"https://webc.org/runner/wasi/command@unstable_\", \"annotations\": {\"wasi\": {\"atom\": \"wasmer-pack\", \"package\": \"wasmer/wasmer-pack-cli\", \"main_args\": null}}}}, \"entrypoint\": \"wasmer-pack\"}",
                    "piritaDownloadUrl": "https://storage.googleapis.com/wapm-registry-prod/webc/wasmer/wasmer-pack-cli/0.5.0/wasmer-pack-cli-0.5.0.webc",
                    "piritaSha256Hash": "d30ca468372faa96469163d2d1546dd34be9505c680677e6ab86a528a268e5f5"
                }
              },
              {
                "version": "0.5.0-rc.1",
                "isArchived": false,
                "v2": {
                    "webcManifest": "{\"atoms\": {\"wasmer-pack\": {\"kind\": \"https://webc.org/kind/wasm\", \"signature\": \"sha256:ThybHIc2elJEcDdQiq5ffT1TVaNs70+WAqoKw4Tkh3E=\"}}, \"package\": {\"wapm\": {\"name\": \"wasmer/wasmer-pack-cli\", \"readme\": {\"path\": \"README.md\", \"volume\": \"metadata\"}, \"license\": \"MIT\", \"version\": \"0.5.0-rc.1\", \"homepage\": \"https://wasmer.io/\", \"repository\": \"https://github.com/wasmerio/wasmer-pack\", \"description\": \"A code generator that lets you treat WebAssembly modules like native dependencies.\"}}, \"commands\": {\"wasmer-pack\": {\"runner\": \"https://webc.org/runner/wasi/command@unstable_\", \"annotations\": {\"wasi\": {\"atom\": \"wasmer-pack\", \"package\": \"wasmer/wasmer-pack-cli\", \"main_args\": null}}}}, \"entrypoint\": \"wasmer-pack\"}",
                    "piritaDownloadUrl": "https://storage.googleapis.com/wapm-registry-prod/webc/wasmer/wasmer-pack-cli/0.5.0-rc.1/wasmer-pack-cli-0.5.0-rc.1.webc",
                    "piritaSha256Hash": "0cd5d6e4c33c92c52784afed3a60c056953104d719717948d4663ff2521fe2bb"
                },
                "v3": {
                    "webcManifest": "{\"atoms\": {\"wasmer-pack\": {\"kind\": \"https://webc.org/kind/wasm\", \"signature\": \"sha256:ThybHIc2elJEcDdQiq5ffT1TVaNs70+WAqoKw4Tkh3E=\"}}, \"package\": {\"wapm\": {\"name\": \"wasmer/wasmer-pack-cli\", \"readme\": {\"path\": \"README.md\", \"volume\": \"metadata\"}, \"license\": \"MIT\", \"version\": \"0.5.0-rc.1\", \"homepage\": \"https://wasmer.io/\", \"repository\": \"https://github.com/wasmerio/wasmer-pack\", \"description\": \"A code generator that lets you treat WebAssembly modules like native dependencies.\"}}, \"commands\": {\"wasmer-pack\": {\"runner\": \"https://webc.org/runner/wasi/command@unstable_\", \"annotations\": {\"wasi\": {\"atom\": \"wasmer-pack\", \"package\": \"wasmer/wasmer-pack-cli\", \"main_args\": null}}}}, \"entrypoint\": \"wasmer-pack\"}",
                    "piritaDownloadUrl": "https://storage.googleapis.com/wapm-registry-prod/webc/wasmer/wasmer-pack-cli/0.5.0-rc.1/wasmer-pack-cli-0.5.0-rc.1.webc",
                    "piritaSha256Hash": "0cd5d6e4c33c92c52784afed3a60c056953104d719717948d4663ff2521fe2bb"
                }
              }
            ]
          },
          "info": {
            "defaultFrontend": "https://wasmer.io"
          }
        }
      }
    "#;

    #[derive(Debug)]
    struct DummyClient {
        requests: Mutex<Vec<HttpRequest>>,
        responses: Mutex<Vec<HttpResponse>>,
    }

    impl DummyClient {
        fn new(responses: Vec<HttpResponse>) -> Self {
            DummyClient {
                requests: Mutex::new(Vec::new()),
                responses: Mutex::new(responses),
            }
        }

        fn take_requests(&self) -> Vec<HttpRequest> {
            std::mem::take(&mut *self.requests.lock().unwrap())
        }
    }

    impl HttpClient for DummyClient {
        fn request(
            &self,
            request: HttpRequest,
        ) -> futures::future::BoxFuture<'_, Result<HttpResponse, anyhow::Error>> {
            self.requests.lock().unwrap().push(request);
            let response = self.responses.lock().unwrap().remove(0);
            Box::pin(async { Ok(response) })
        }
    }

    #[tokio::test]
    async fn run_known_query() {
        let response = HttpResponse {
            body: Some(WASMER_PACK_CLI_RESPONSE.to_vec()),
            redirected: false,
            status: StatusCode::OK,
            headers: HeaderMap::new(),
        };
        let client = Arc::new(DummyClient::new(vec![response]));
        let registry_endpoint = BackendSource::WASMER_PROD_ENDPOINT.parse().unwrap();
        let request = PackageSource::from_str("wasmer/wasmer-pack-cli@^0.6").unwrap();
        let source = BackendSource::new(registry_endpoint, client.clone());

        let summaries = source.query(&request).await.unwrap();

        assert_eq!(
            summaries,
            [PackageSummary {
                pkg: PackageInfo {
                    id: PackageId::new_named("wasmer/wasmer-pack-cli", Version::new(0, 6, 0)),
                    dependencies: Vec::new(),
                    commands: vec![crate::runtime::resolver::Command {
                        name: "wasmer-pack".to_string(),
                    },],
                    entrypoint: Some("wasmer-pack".to_string()),
                    filesystem: vec![],
                },
                dist: DistributionInfo {
                    webc: "https://storage.googleapis.com/wapm-registry-prod/webc/wasmer/wasmer-pack-cli/0.6.0/wasmer-pack-cli-0.6.0.webc"
                        .parse()
                        .unwrap(),
                    webc_sha256: WebcHash::from_bytes([
                        126, 26, 221, 22, 64, 208, 3, 127, 246, 167, 38, 205, 126, 20, 234, 54, 21,
                        158, 194, 219, 140, 182, 222, 189, 14, 66, 250, 39, 57, 190, 165, 43,
                    ]),
                }
            }]
        );
        let requests = client.take_requests();
        assert_eq!(requests.len(), 1);
        let request = &requests[0];
        assert_eq!(request.method, http::Method::POST);
        assert_eq!(request.url.as_str(), BackendSource::WASMER_PROD_ENDPOINT);
        assert_eq!(request.headers.len(), 2);
        assert_eq!(request.headers["User-Agent"], USER_AGENT);
        assert_eq!(request.headers["Content-Type"], "application/json");
        let body: serde_json::Value =
            serde_json::from_slice(request.body.as_deref().unwrap()).unwrap();
        let expected_body: serde_json::Value =
            serde_json::from_slice(WASMER_PACK_CLI_REQUEST).unwrap();
        assert_eq!(body, expected_body);
    }

    /// For the full context, see #3946 on GitHub or the original conversation
    /// [on
    /// Slack](https://wasmerio.slack.com/archives/C03MX4KL6KH/p1685706988500919).
    #[tokio::test]
    async fn skip_package_versions_with_missing_fields() {
        let body = serde_json::json! {
            {
                "data": {
                    "getPackage": {
                        "packageName": "cowsay",
                        "namespace": "_",
                        "versions": [
                            {
                                "version": "0.2.0",
                                "v2": {
                                    "piritaDownloadUrl": "https://storage.googleapis.com/wapm-registry-prod/packages/_/cowsay/cowsay-0.2.0.webc",
                                    "piritaSha256Hash": "9586938a0a89219dafe4ae97a901c56d4b3e2a9941520d1309ae880c9a1868c9",
                                    "webcManifest": "{\"atoms\": {\"cowsay\": {\"kind\": \"https://webc.org/kind/wasm\", \"signature\": \"sha256:DPmhiSNXCg5261eTUi3BIvAc/aJttGj+nD+bGhQkVQo=\"}}, \"package\": {\"wapm\": {\"name\": \"cowsay\", \"readme\": {\"path\": \"README.md\", \"volume\": \"metadata\"}, \"version\": \"0.2.0\", \"repository\": \"https://github.com/wapm-packages/cowsay\", \"description\": \"cowsay is a program that generates ASCII pictures of a cow with a message\"}}, \"commands\": {\"cowsay\": {\"runner\": \"https://webc.org/runner/wasi/command@unstable_\", \"annotations\": {\"wasi\": {\"atom\": \"cowsay\", \"package\": null, \"main_args\": null}}}, \"cowthink\": {\"runner\": \"https://webc.org/runner/wasi/command@unstable_\", \"annotations\": {\"wasi\": {\"atom\": \"cowsay\", \"package\": null, \"main_args\": null}}}}}",
                                },
                                "v3": {
                                    "piritaDownloadUrl": "https://storage.googleapis.com/wapm-registry-prod/packages/_/cowsay/cowsay-0.2.0.webc",
                                    "piritaSha256Hash": "9586938a0a89219dafe4ae97a901c56d4b3e2a9941520d1309ae880c9a1868c9",
                                    "webcManifest": "{\"atoms\": {\"cowsay\": {\"kind\": \"https://webc.org/kind/wasm\", \"signature\": \"sha256:DPmhiSNXCg5261eTUi3BIvAc/aJttGj+nD+bGhQkVQo=\"}}, \"package\": {\"wapm\": {\"name\": \"cowsay\", \"readme\": {\"path\": \"README.md\", \"volume\": \"metadata\"}, \"version\": \"0.2.0\", \"repository\": \"https://github.com/wapm-packages/cowsay\", \"description\": \"cowsay is a program that generates ASCII pictures of a cow with a message\"}}, \"commands\": {\"cowsay\": {\"runner\": \"https://webc.org/runner/wasi/command@unstable_\", \"annotations\": {\"wasi\": {\"atom\": \"cowsay\", \"package\": null, \"main_args\": null}}}, \"cowthink\": {\"runner\": \"https://webc.org/runner/wasi/command@unstable_\", \"annotations\": {\"wasi\": {\"atom\": \"cowsay\", \"package\": null, \"main_args\": null}}}}}",
                                }
                            },
                            {
                                "version": "0.1.3",
                                "v2": {
                                    "piritaDownloadUrl": "https://example.com/",
                                    "piritaSha256Hash": "1234asdf"
                                },
                                "v3": {
                                    "piritaDownloadUrl": "https://example.com/",
                                    "piritaSha256Hash": "1234asdf"
                                }
                            },
                            {
                                "version": "0.1.2",
                                "v2": {
                                    "piritaDownloadUrl": "https://example.com/",
                                    "piritaSha256Hash": "1234asdf",
                                    "webcManifest": "{}",
                                },
                                "v3": {
                                    "piritaDownloadUrl": "https://example.com/",
                                    "piritaSha256Hash": "1234asdf",
                                    "webcManifest": "{}",
                                }
                            },
                            {
                                "version": "0.1.3",
                                "v2": {
                                    "webcManifest": "{}",
                                    "piritaDownloadUrl": "https://example.com/",
                                    "piritaSha256Hash": "1234asdf"
                                },
                                "v3": {
                                    "webcManifest": "{}",
                                    "piritaDownloadUrl": "https://example.com/",
                                    "piritaSha256Hash": "1234asdf"
                                }
                            }
                        ]
                    },
                    "info": {
                        "defaultFrontend": "https://wasmer.io/",
                    },
                }
            }

        };
        let response = HttpResponse {
            body: Some(serde_json::to_vec(&body).unwrap()),
            redirected: false,
            status: StatusCode::OK,
            headers: HeaderMap::new(),
        };
        let client = Arc::new(DummyClient::new(vec![response]));
        let registry_endpoint = BackendSource::WASMER_PROD_ENDPOINT.parse().unwrap();
        let request = PackageSource::from_str("_/cowsay").unwrap();
        let source = BackendSource::new(registry_endpoint, client.clone());

        let summaries = source.query(&request).await.unwrap();

        assert_eq!(summaries.len(), 1);
        assert_eq!(
            summaries[0].pkg.id.as_named().unwrap().version.to_string(),
            "0.2.0"
        );
    }

    #[tokio::test]
    async fn skip_archived_package_versions() {
        let body = serde_json::json! {
            {
                "data": {
                    "getPackage": {
                        "packageName": "python",
                        "namespace": "wasmer",
                        "versions": [
                            {
                                "version": "3.12.2",
                                "isArchived": true,
                                "v2": {
                                    "webcManifest": "{\"atoms\": {\"python\": {\"kind\": \"https://webc.org/kind/wasm\", \"signature\": \"sha256:ibsq6QL4qB4GtCE8IA2yfHVwI4fLoIGXsALsAx16y5M=\"}}, \"package\": {\"wapm\": {\"name\": \"wasmer/python\", \"license\": \"ISC\", \"version\": \"3.12.2\", \"repository\": \"https://github.com/wapm-packages/python\", \"description\": \"Python is an interpreted, high-level, general-purpose programming language\"}}, \"commands\": {\"python\": {\"runner\": \"https://webc.org/runner/wasi/command@unstable_\", \"annotations\": {\"wasi\": {\"atom\": \"python\", \"package\": null, \"main_args\": null}}}}, \"entrypoint\": \"python\"}",
                                    "piritaDownloadUrl": "https://storage.googleapis.com/wapm-registry-prod/packages/wasmer/python/python-3.12.0-build.5-a11e0414-c68d-473c-958f-fc96ef7adb20.webc",
                                    "piritaSha256Hash": "7771ed54376c16da86581736fad84fb761a049915902a7070e854965be0d5874"
                                },
                                "v3": {
                                    "webcManifest": "{\"atoms\": {\"python\": {\"kind\": \"https://webc.org/kind/wasm\", \"signature\": \"sha256:ibsq6QL4qB4GtCE8IA2yfHVwI4fLoIGXsALsAx16y5M=\"}}, \"package\": {\"wapm\": {\"name\": \"wasmer/python\", \"license\": \"ISC\", \"version\": \"3.12.2\", \"repository\": \"https://github.com/wapm-packages/python\", \"description\": \"Python is an interpreted, high-level, general-purpose programming language\"}}, \"commands\": {\"python\": {\"runner\": \"https://webc.org/runner/wasi/command@unstable_\", \"annotations\": {\"wasi\": {\"atom\": \"python\", \"package\": null, \"main_args\": null}}}}, \"entrypoint\": \"python\"}",
                                    "piritaDownloadUrl": "https://storage.googleapis.com/wapm-registry-prod/packages/wasmer/python/python-3.12.0-build.5-a11e0414-c68d-473c-958f-fc96ef7adb20.webc",
                                    "piritaSha256Hash": "7771ed54376c16da86581736fad84fb761a049915902a7070e854965be0d5874"
                                }
                            },
                            {
                                "version": "3.12.1",
                                "isArchived": false,
                                "v2": {
                                    "webcManifest": "{\"atoms\": {\"python\": {\"kind\": \"https://webc.org/kind/wasm\", \"signature\": \"sha256:O36BXLHv3/80cABbAiF7gzuSHzzin1blTfJ42LDhT18=\"}}, \"package\": {\"wapm\": {\"name\": \"wasmer/python\", \"license\": \"ISC\", \"version\": \"3.12.1\", \"repository\": \"https://github.com/wapm-packages/python\", \"description\": \"Python is an interpreted, high-level, general-purpose programming language\"}}, \"commands\": {\"python\": {\"runner\": \"https://webc.org/runner/wasi/command@unstable_\", \"annotations\": {\"wasi\": {\"atom\": \"python\", \"package\": null, \"main_args\": null}}}}, \"entrypoint\": \"python\"}",
                                    "piritaDownloadUrl": "https://storage.googleapis.com/wapm-registry-prod/packages/wasmer/python/python-3.12.0-build.2-ed98c999-fcda-4f80-96dc-7c0f8be8baa6.webc",
                                    "piritaSha256Hash": "7835401e3ca1977ba05b5e51541363783b8a7700da270dd851f10fe2e4f27f07"
                                },
                                "v3": {
                                    "webcManifest": "{\"atoms\": {\"python\": {\"kind\": \"https://webc.org/kind/wasm\", \"signature\": \"sha256:O36BXLHv3/80cABbAiF7gzuSHzzin1blTfJ42LDhT18=\"}}, \"package\": {\"wapm\": {\"name\": \"wasmer/python\", \"license\": \"ISC\", \"version\": \"3.12.1\", \"repository\": \"https://github.com/wapm-packages/python\", \"description\": \"Python is an interpreted, high-level, general-purpose programming language\"}}, \"commands\": {\"python\": {\"runner\": \"https://webc.org/runner/wasi/command@unstable_\", \"annotations\": {\"wasi\": {\"atom\": \"python\", \"package\": null, \"main_args\": null}}}}, \"entrypoint\": \"python\"}",
                                    "piritaDownloadUrl": "https://storage.googleapis.com/wapm-registry-prod/packages/wasmer/python/python-3.12.0-build.2-ed98c999-fcda-4f80-96dc-7c0f8be8baa6.webc",
                                    "piritaSha256Hash": "7835401e3ca1977ba05b5e51541363783b8a7700da270dd851f10fe2e4f27f07"
                                }
                            },
                            {
                                "version": "3.12.0",
                                "isArchived": true,
                                "v2": {
                                    "webcManifest": "{\"atoms\": {\"python\": {\"kind\": \"https://webc.org/kind/wasm\", \"signature\": \"sha256:O36BXLHv3/80cABbAiF7gzuSHzzin1blTfJ42LDhT18=\"}}, \"package\": {\"wapm\": {\"name\": \"wasmer/python\", \"license\": \"ISC\", \"version\": \"3.12.0\", \"repository\": \"https://github.com/wapm-packages/python\", \"description\": \"Python is an interpreted, high-level, general-purpose programming language\"}}, \"commands\": {\"python\": {\"runner\": \"https://webc.org/runner/wasi/command@unstable_\", \"annotations\": {\"wasi\": {\"atom\": \"python\", \"package\": null, \"main_args\": null}}}}, \"entrypoint\": \"python\"}",
                                    "piritaDownloadUrl": "https://storage.googleapis.com/wapm-registry-prod/packages/wasmer/python/python-3.12.0-32065e5e-84fe-4483-a380-0aa750772a3a.webc",
                                    "piritaSha256Hash": "e5d6e9d16db988eb323e34e2c152ebfb32dc7043d6b7ddc00ad57d3beae24adb"
                                },
                                "v3": {
                                    "webcManifest": "{\"atoms\": {\"python\": {\"kind\": \"https://webc.org/kind/wasm\", \"signature\": \"sha256:O36BXLHv3/80cABbAiF7gzuSHzzin1blTfJ42LDhT18=\"}}, \"package\": {\"wapm\": {\"name\": \"wasmer/python\", \"license\": \"ISC\", \"version\": \"3.12.0\", \"repository\": \"https://github.com/wapm-packages/python\", \"description\": \"Python is an interpreted, high-level, general-purpose programming language\"}}, \"commands\": {\"python\": {\"runner\": \"https://webc.org/runner/wasi/command@unstable_\", \"annotations\": {\"wasi\": {\"atom\": \"python\", \"package\": null, \"main_args\": null}}}}, \"entrypoint\": \"python\"}",
                                    "piritaDownloadUrl": "https://storage.googleapis.com/wapm-registry-prod/packages/wasmer/python/python-3.12.0-32065e5e-84fe-4483-a380-0aa750772a3a.webc",
                                    "piritaSha256Hash": "e5d6e9d16db988eb323e34e2c152ebfb32dc7043d6b7ddc00ad57d3beae24adb"
                                }
                            },
                        ]
                    },
                    "info": {
                        "defaultFrontend": "https://wasmer.io/",
                    },
                }
            }
        };
        let response = HttpResponse {
            body: Some(serde_json::to_vec(&body).unwrap()),
            redirected: false,
            status: StatusCode::OK,
            headers: HeaderMap::new(),
        };
        let client = Arc::new(DummyClient::new(vec![response]));
        let registry_endpoint = BackendSource::WASMER_PROD_ENDPOINT.parse().unwrap();
        let request = PackageSource::from_str("wasmer/python").unwrap();
        let source = BackendSource::new(registry_endpoint, client.clone());

        let summaries = source.query(&request).await.unwrap();

        assert_eq!(summaries.len(), 1);
        assert_eq!(
            summaries[0].pkg.id.as_named().unwrap().version.to_string(),
            "3.12.1"
        );
    }

    #[tokio::test]
    async fn query_the_backend_again_if_cached_queries_dont_match() {
        let cached_value = serde_json::from_value(serde_json::json! {
            {
                "data": {
                    "getPackage": {
                        "packageName": "python",
                        "namespace": "wasmer",
                        "versions": [
                            {
                                "version": "3.12.0",
                                "v2": {
                                    "webcManifest": "{\"package\": {\"wapm\": {\"name\": \"wasmer/python\", \"version\": \"3.12.0\", \"description\": \"Python\"}}}",
                                    "piritaDownloadUrl": "https://wasmer.io/wasmer/python@3.12.0",
                                    "piritaSha256Hash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                                },
                                "v3": {
                                    "webcManifest": "{\"package\": {\"wapm\": {\"name\": \"wasmer/python\", \"version\": \"3.12.0\", \"description\": \"Python\"}}}",
                                    "piritaDownloadUrl": "https://wasmer.io/wasmer/python@3.12.0",
                                    "piritaSha256Hash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                                }
                            },
                        ]
                    },
                    "info": {
                        "defaultFrontend": "https://wasmer.io/",
                    },
                }
            }
        }).unwrap();
        let body = serde_json::json! {
            {
                "data": {
                    "getPackage": {
                        "packageName": "python",
                        "namespace": "wasmer",
                        "versions": [
                            {
                                "version": "4.0.0",
                                "v2": {
                                    "webcManifest": "{\"package\": {\"wapm\": {\"name\": \"wasmer/python\", \"version\": \"4.0.0\", \"description\": \"Python\"}}}",
                                    "piritaDownloadUrl": "https://wasmer.io/wasmer/python@4.0.0",
                                    "piritaSha256Hash": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                                },
                                "v3": {
                                    "webcManifest": "{\"package\": {\"wapm\": {\"name\": \"wasmer/python\", \"version\": \"4.0.0\", \"description\": \"Python\"}}}",
                                    "piritaDownloadUrl": "https://wasmer.io/wasmer/python@4.0.0",
                                    "piritaSha256Hash": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                                }
                            },
                            {
                                "version": "3.12.0",
                                "v2": {
                                    "webcManifest": "{\"package\": {\"wapm\": {\"name\": \"wasmer/python\", \"version\": \"3.12.0\", \"description\": \"Python\"}}}",
                                    "piritaDownloadUrl": "https://wasmer.io/wasmer/python@3.12.0",
                                    "piritaSha256Hash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                                },
                                "v3": {
                                    "webcManifest": "{\"package\": {\"wapm\": {\"name\": \"wasmer/python\", \"version\": \"3.12.0\", \"description\": \"Python\"}}}",
                                    "piritaDownloadUrl": "https://wasmer.io/wasmer/python@3.12.0",
                                    "piritaSha256Hash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                                }
                            },
                        ]
                    },
                    "info": {
                        "defaultFrontend": "https://wasmer.io/",
                    },
                }
            }
        };
        let response = HttpResponse {
            body: Some(serde_json::to_vec(&body).unwrap()),
            redirected: false,
            status: StatusCode::OK,
            headers: HeaderMap::new(),
        };
        let client = Arc::new(DummyClient::new(vec![response]));
        let registry_endpoint = BackendSource::WASMER_PROD_ENDPOINT.parse().unwrap();
        let request = PackageSource::from_str("wasmer/python@4.0.0").unwrap();
        let temp = tempfile::tempdir().unwrap();
        let source = BackendSource::new(registry_endpoint, client.clone())
            .with_local_cache(temp.path(), Duration::from_secs(0));
        source
            .cache
            .as_ref()
            .unwrap()
            .update("wasmer/python", &cached_value)
            .unwrap();

        let summaries = source.query(&request).await.unwrap();

        assert_eq!(summaries.len(), 1);
        assert_eq!(
            summaries[0].pkg.id.as_named().unwrap().version.to_string(),
            "4.0.0"
        );
    }
}
