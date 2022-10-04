use std::path::{Path, PathBuf};

pub mod queries {
    
    use graphql_client::*;

    #[derive(GraphQLQuery)]
    #[graphql(
        schema_path = "graphql/schema.graphql",
        query_path = "graphql/queries/get_package_version.graphql",
        response_derives = "Debug"
    )]
    pub(crate) struct GetPackageVersionQuery;

    #[derive(GraphQLQuery)]
    #[graphql(
        schema_path = "graphql/schema.graphql",
        query_path = "graphql/queries/get_package.graphql",
        response_derives = "Debug"
    )]
    pub(crate) struct GetPackageQuery;

    #[derive(GraphQLQuery)]
    #[graphql(
        schema_path = "graphql/schema.graphql",
        query_path = "graphql/queries/test_if_registry_present.graphql",
        response_derives = "Debug"
    )]
    pub(crate) struct TestIfRegistryPresent;
}

#[derive(Debug, Clone, PartialEq, PartialOrd, Eq, Ord)]
pub struct PackageDownloadInfo {
    pub package: String,
    pub command: String,
    pub url: String,
}

pub fn get_command_local(name: &str) -> Result<PathBuf, String> {
    Err(format!("unimplemented"))
}

pub fn get_package_local(name: &str, version: Option<&str>) -> Result<PathBuf, String> {
    Err(format!("unimplemented"))
}

pub fn query_command_from_registry(name: &str) -> Result<PackageDownloadInfo, String> {
    Err(format!("unimplemented"))
}

pub fn query_package_from_registry(
    name: &str,
    version: Option<&str>,
) -> Result<PackageDownloadInfo, String> {
    Err(format!("unimplemented"))
}

/// Returs the path to the directory where all packages on this computer are being stored
pub fn get_global_install_dir(registry_host: &str) -> Option<PathBuf> {
    Some(
        dirs::home_dir()?
            .join(".wasmer")
            .join("checkouts")
            .join(registry_host),
    )
}

pub fn download_and_unpack_targz(url: &str, target_path: &Path) -> Result<PathBuf, String> {
    Err(format!("unimplemented"))
}

pub fn install_package(name: &str, version: Option<&str>) -> Result<PathBuf, String> {
    std::thread::sleep(std::time::Duration::from_secs(4));
    Err(format!("unimplemented"))
}

pub fn test_if_registry_present(url: &str) -> Result<bool, String> {
    Ok(false)
}

pub fn get_all_available_registries() -> Result<Vec<String>, String> {
    Ok(Vec::new())
}
