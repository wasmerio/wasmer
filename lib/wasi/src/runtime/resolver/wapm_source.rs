use std::sync::Arc;

use anyhow::{Context, Error};
use semver::Version;
use url::Url;
use webc::metadata::{Manifest, UrlOrManifest};

use crate::{
    http::{HttpClient, HttpRequest, HttpResponse},
    runtime::resolver::{Dependency, PackageSpecifier, Source, SourceId, SourceKind, Summary},
};

/// A [`Source`] which will resolve dependencies by pinging a WAPM-like GraphQL
/// endpoint.
#[derive(Debug, Clone)]
pub struct WapmSource {
    registry_endpoint: Url,
    client: Arc<dyn HttpClient + Send + Sync>,
}

impl WapmSource {
    pub const WAPM_DEV_ENDPOINT: &str = "https://registry.wapm.dev/graphql";
    pub const WAPM_PROD_ENDPOINT: &str = "https://registry.wapm.io/graphql";

    pub fn new(registry_endpoint: Url, client: Arc<dyn HttpClient + Send + Sync>) -> Self {
        WapmSource {
            registry_endpoint,
            client,
        }
    }
}

#[async_trait::async_trait]
impl Source for WapmSource {
    fn id(&self) -> SourceId {
        SourceId::new(SourceKind::Registry, self.registry_endpoint.clone())
    }

    async fn query(&self, package: &PackageSpecifier) -> Result<Vec<Summary>, Error> {
        let (full_name, version_constraint) = match package {
            PackageSpecifier::Registry { full_name, version } => (full_name, version),
            _ => return Ok(Vec::new()),
        };

        let request = HttpRequest {
            url: self.registry_endpoint.to_string(),
            method: "GET".to_string(),
            body: Some(WAPM_WEBC_QUERY_ALL.replace("$NAME", full_name).into_bytes()),
            headers: vec![(
                "User-Agent".to_string(),
                crate::http::USER_AGENT.to_string(),
            )],
            options: Default::default(),
        };

        let HttpResponse {
            ok,
            status,
            status_text,
            body,
            ..
        } = self.client.request(request).await?;

        if !ok {
            let url = &self.registry_endpoint;
            anyhow::bail!("\"{url}\" replied with {status} {status_text}");
        }

        let body = body.unwrap_or_default();
        let response: WapmWebQuery =
            serde_json::from_slice(&body).context("Unable to deserialize the response")?;

        let mut summaries = Vec::new();

        for pkg_version in response.data.get_package.versions {
            let version = Version::parse(&pkg_version.version)?;
            if version_constraint.matches(&version) {
                let summary = decode_summary(pkg_version, full_name.clone(), self.id())?;
                summaries.push(summary);
            }
        }

        Ok(summaries)
    }
}

fn decode_summary(
    pkg_version: WapmWebQueryGetPackageVersion,
    package_name: String,
    source: SourceId,
) -> Result<Summary, Error> {
    let WapmWebQueryGetPackageVersion {
        version,
        manifest,
        distribution:
            WapmWebQueryGetPackageVersionDistribution {
                pirita_download_url,
                pirita_sha256_hash,
            },
    } = pkg_version;

    let manifest: Manifest = serde_json::from_slice(manifest.as_bytes())
        .context("Unable to deserialize the manifest")?;

    let mut webc_sha256 = [0_u8; 32];
    hex::decode_to_slice(&pirita_sha256_hash, &mut webc_sha256)?;

    let dependencies = manifest
        .use_map
        .iter()
        .map(|(alias, value)| parse_dependency(alias, value))
        .collect::<Result<Vec<_>, _>>()?;

    let commands = manifest
        .commands
        .iter()
        .map(|(name, _value)| crate::runtime::resolver::Command {
            name: name.to_string(),
        })
        .collect();

    Ok(Summary {
        package_name,
        version: version.parse()?,
        webc: pirita_download_url.parse()?,
        webc_sha256,
        dependencies,
        commands,
        source,
    })
}

fn parse_dependency(_alias: &str, _value: &UrlOrManifest) -> Result<Dependency, Error> {
    todo!();
}

#[allow(dead_code)]
pub const WAPM_WEBC_QUERY_ALL: &str = r#"{
    getPackage(name: "$NAME") {
        versions {
        version
        piritaManifest
        distribution {
            piritaDownloadUrl
            piritaSha256Hash
        }
        }
    }
}"#;

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct WapmWebQuery {
    #[serde(rename = "data")]
    pub data: WapmWebQueryData,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct WapmWebQueryData {
    #[serde(rename = "getPackage")]
    pub get_package: WapmWebQueryGetPackage,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct WapmWebQueryGetPackage {
    pub versions: Vec<WapmWebQueryGetPackageVersion>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct WapmWebQueryGetPackageVersion {
    pub version: String,
    #[serde(rename = "piritaManifest")]
    pub manifest: String,
    pub distribution: WapmWebQueryGetPackageVersionDistribution,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct WapmWebQueryGetPackageVersionDistribution {
    #[serde(rename = "piritaDownloadUrl")]
    pub pirita_download_url: String,
    #[serde(rename = "piritaSha256Hash")]
    pub pirita_sha256_hash: String,
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    const WASMER_PACK_CLI_QUERY: &str = r#"{
    getPackage(name: "wasmer/wasmer-pack-cli") {
        versions {
        version
        piritaManifest
        distribution {
            piritaDownloadUrl
            piritaSha256Hash
        }
        }
    }
}"#;
    const WASMER_PACK_CLI_RESPONSE: &[u8] = include_bytes!("wasmer_pack_cli_response.json");

    #[derive(Debug, Default)]
    struct DummyClient;

    impl HttpClient for DummyClient {
        fn request(
            &self,
            request: HttpRequest,
        ) -> futures::future::BoxFuture<'_, Result<HttpResponse, anyhow::Error>> {
            let body = String::from_utf8(request.body.unwrap()).unwrap();
            assert_eq!(body, WASMER_PACK_CLI_QUERY);
            assert_eq!(request.url, WapmSource::WAPM_PROD_ENDPOINT);
            let headers: HashMap<String, String> = request.headers.into_iter().collect();
            assert_eq!(headers.len(), 1);
            assert_eq!(headers["User-Agent"], crate::http::USER_AGENT);

            Box::pin(async {
                Ok(HttpResponse {
                    pos: 0,
                    body: Some(WASMER_PACK_CLI_RESPONSE.to_vec()),
                    ok: true,
                    redirected: false,
                    status: 200,
                    status_text: "OK".to_string(),
                    headers: Vec::new(),
                })
            })
        }
    }

    #[tokio::test]
    async fn run_known_query() {
        let client = Arc::new(DummyClient::default());
        let registry_endpoint = WapmSource::WAPM_PROD_ENDPOINT.parse().unwrap();
        let request = PackageSpecifier::Registry {
            full_name: "wasmer/wasmer-pack-cli".to_string(),
            version: "^0.6".parse().unwrap(),
        };
        let source = WapmSource::new(registry_endpoint, client);

        let summaries = source.query(&request).await.unwrap();

        assert_eq!(
            summaries,
            [Summary {
                package_name: "wasmer/wasmer-pack-cli".to_string(),
                version: Version::new(0, 6, 0),
                webc: "https://registry-cdn.wapm.io/packages/wasmer/wasmer-pack-cli/wasmer-pack-cli-0.6.0-654a2ed8-875f-11ed-90e2-c6aeb50490de.webc".parse().unwrap(),
                webc_sha256: [
                    126,
                    26,
                    221,
                    22,
                    64,
                    208,
                    3,
                    127,
                    246,
                    167,
                    38,
                    205,
                    126,
                    20,
                    234,
                    54,
                    21,
                    158,
                    194,
                    219,
                    140,
                    182,
                    222,
                    189,
                    14,
                    66,
                    250,
                    39,
                    57,
                    190,
                    165,
                    43,
                ],
                dependencies: Vec::new(),
                commands: vec![
                    crate::runtime::resolver::Command {
                        name: "wasmer-pack".to_string(),
                    },
                ],
                source: source.id(),
            }]
        );
    }
}
