use serde::*;

pub const WAPM_WEBC_URL: &str = "https://registry.wapm.dev/graphql?query=";
#[allow(dead_code)]
pub const WAPM_WEBC_QUERY_ALL: &str = r#"
{
    getPackage(name: "<NAME>") {
        versions {
            version,
            distribution {
                downloadUrl,
                piritaDownloadUrl
            }
        }
    }
}"#;
pub const WAPM_WEBC_QUERY_LAST: &str = r#"
{
    getPackage(name: "<NAME>") {
        lastVersion {
            version,
            distribution {
                downloadUrl,
                piritaDownloadUrl
            }
        }
    }
}"#;
pub const WAPM_WEBC_QUERY_SPECIFIC: &str = r#"
{
    getPackageVersion(name: "<NAME>", version: "<VERSION>") {
        version,
        distribution {
            downloadUrl,
            piritaDownloadUrl
        }
    }
}"#;
pub const WAPM_WEBC_QUERY_TAG: &str = "<NAME>";
pub const WAPM_WEBC_VERSION_TAG: &str = "<VERSION>";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WapmWebQueryGetPackageLastVersionDistribution {
    #[serde(rename = "downloadUrl")]
    pub download_url: Option<String>,
    #[serde(rename = "piritaDownloadUrl")]
    pub pirita_download_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WapmWebQueryGetPackageVersion {
    #[serde(rename = "version")]
    pub version: String,
    #[serde(rename = "distribution")]
    pub distribution: WapmWebQueryGetPackageLastVersionDistribution,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WapmWebQueryGetPackage {
    #[serde(rename = "lastVersion")]
    pub last_version: WapmWebQueryGetPackageVersion,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WapmWebQueryData {
    #[serde(rename = "getPackage")]
    pub get_package: Option<WapmWebQueryGetPackage>,
    #[serde(rename = "getPackageVersion")]
    pub get_package_version: Option<WapmWebQueryGetPackageVersion>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WapmWebQuery {
    #[serde(rename = "data")]
    pub data: WapmWebQueryData,
}
