pub use queries::*;

pub use cynic::Id;

#[cynic::schema_for_derives(file = r#"schema.graphql"#, module = "schema")]
mod queries {
    use serde::Serialize;
    use time::OffsetDateTime;

    use super::schema;

    #[derive(cynic::Scalar, Debug, Clone)]
    pub struct DateTime(pub String);

    impl TryFrom<OffsetDateTime> for DateTime {
        type Error = time::error::Format;

        fn try_from(value: OffsetDateTime) -> Result<Self, Self::Error> {
            value
                .format(&time::format_description::well_known::Rfc3339)
                .map(Self)
        }
    }

    impl TryFrom<DateTime> for OffsetDateTime {
        type Error = time::error::Parse;

        fn try_from(value: DateTime) -> Result<Self, Self::Error> {
            OffsetDateTime::parse(&value.0, &time::format_description::well_known::Rfc3339)
        }
    }

    #[derive(cynic::Scalar, Debug, Clone)]
    pub struct JSONString(pub String);

    #[derive(cynic::Enum, Clone, Copy, Debug)]
    pub enum GrapheneRole {
        Owner,
        Admin,
        Editor,
        Viewer,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct GetCurrentUserVars {
        pub namespace_role: Option<GrapheneRole>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetCurrentUserVars")]
    pub struct GetCurrentUser {
        pub viewer: Option<UserWithNamespaces>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct User {
        pub id: cynic::Id,
        pub username: String,
    }

    #[derive(cynic::QueryFragment, Debug, Clone)]
    pub struct Package {
        pub id: cynic::Id,
        pub package_name: String,
        pub namespace: Option<String>,
        pub last_version: Option<PackageVersion>,
        pub private: bool,
    }

    #[derive(cynic::QueryFragment, Debug, Clone)]
    pub struct PackageDistribution {
        pub pirita_sha256_hash: Option<String>,
        pub pirita_download_url: Option<String>,
        pub download_url: Option<String>,
    }

    #[derive(cynic::QueryFragment, Debug, Clone)]
    pub struct PackageVersion {
        pub id: cynic::Id,
        pub version: String,
        pub created_at: DateTime,
        pub distribution: PackageDistribution,
    }

    #[derive(cynic::QueryFragment, Debug, Clone)]
    #[cynic(graphql_type = "PackageVersion")]
    pub struct PackageVersionWithPackage {
        pub id: cynic::Id,
        pub version: String,
        pub created_at: DateTime,
        pub pirita_manifest: Option<JSONString>,
        pub distribution: PackageDistribution,

        pub package: Package,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct GetPackageVars {
        pub name: String,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetPackageVars")]
    pub struct GetPackage {
        #[arguments(name: $name)]
        pub get_package: Option<Package>,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct GetPackageVersionVars {
        pub name: String,
        pub version: String,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetPackageVersionVars")]
    pub struct GetPackageVersion {
        #[arguments(name: $name, version: $version)]
        pub get_package_version: Option<PackageVersionWithPackage>,
    }

    #[derive(cynic::Enum, Clone, Copy, Debug)]
    pub enum PackageVersionSortBy {
        Newest,
        Oldest,
    }

    #[derive(cynic::QueryVariables, Debug, Clone, Default)]
    pub struct AllPackageVersionsVars {
        pub offset: Option<i32>,
        pub before: Option<String>,
        pub after: Option<String>,
        pub first: Option<i32>,
        pub last: Option<i32>,

        pub created_after: Option<DateTime>,
        pub updated_after: Option<DateTime>,
        pub sort_by: Option<PackageVersionSortBy>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "AllPackageVersionsVars")]
    pub struct GetAllPackageVersions {
        #[arguments(
            first: $first,
            last: $last,
            after: $after,
            before: $before,
            offset: $offset,
            updatedAfter: $updated_after,
            createdAfter: $created_after,
            sortBy: $sort_by,
        )]
        pub all_package_versions: PackageVersionConnection,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct PackageVersionConnection {
        pub page_info: PageInfo,
        pub edges: Vec<Option<PackageVersionEdge>>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct PackageVersionEdge {
        pub node: Option<PackageVersionWithPackage>,
        pub cursor: String,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct GetPackageAndAppVars {
        pub package: String,
        pub app_owner: String,
        pub app_name: String,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetPackageAndAppVars")]
    pub struct GetPackageAndApp {
        #[arguments(name: $package)]
        pub get_package: Option<Package>,
        #[arguments(owner: $app_owner, name: $app_name)]
        pub get_deploy_app: Option<DeployApp>,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct GetCurrentUserWithAppsVars {
        pub after: Option<String>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetCurrentUserWithAppsVars")]
    pub struct GetCurrentUserWithApps {
        pub viewer: Option<UserWithApps>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "User")]
    #[cynic(variables = "GetCurrentUserWithAppsVars")]
    pub struct UserWithApps {
        pub id: cynic::Id,
        pub username: String,
        #[arguments(after: $after)]
        pub apps: DeployAppConnection,
    }

    #[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
    pub struct Owner {
        pub global_name: String,
    }

    #[derive(cynic::QueryFragment, Debug, Clone)]
    #[cynic(graphql_type = "User", variables = "GetCurrentUserVars")]
    pub struct UserWithNamespaces {
        pub id: cynic::Id,
        pub username: String,
        #[arguments(role: $namespace_role)]
        pub namespaces: NamespaceConnection,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct GetUserAppsVars {
        pub username: String,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetUserAppsVars")]
    pub struct GetUserApps {
        #[arguments(username: $username)]
        pub get_user: Option<User>,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct GetDeployAppVars {
        pub name: String,
        pub owner: String,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetDeployAppVars")]
    pub struct GetDeployApp {
        #[arguments(owner: $owner, name: $name)]
        pub get_deploy_app: Option<DeployApp>,
    }

    #[derive(cynic::QueryVariables, Debug, Clone)]
    pub struct PaginationVars {
        pub offset: Option<i32>,
        pub before: Option<String>,
        pub after: Option<String>,
        pub first: Option<i32>,
        pub last: Option<i32>,
    }

    #[derive(cynic::Enum, Clone, Copy, Debug)]
    pub enum DeployAppsSortBy {
        Newest,
        Oldest,
        MostActive,
    }

    #[derive(cynic::QueryVariables, Debug, Clone, Default)]
    pub struct GetDeployAppsVars {
        pub offset: Option<i32>,
        pub before: Option<String>,
        pub after: Option<String>,
        pub first: Option<i32>,
        pub last: Option<i32>,

        pub updated_after: Option<DateTime>,
        pub sort_by: Option<DeployAppsSortBy>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetDeployAppsVars")]
    pub struct GetDeployApps {
        #[arguments(
            first: $first,
            last: $last,
            after: $after,
            before: $before,
            offset: $offset,
            updatedAfter: $updated_after,
            sortBy: $sort_by,
        )]
        pub get_deploy_apps: Option<DeployAppConnection>,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct GetDeployAppByAliasVars {
        pub alias: String,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetDeployAppByAliasVars")]
    pub struct GetDeployAppByAlias {
        #[arguments(alias: $alias)]
        pub get_app_by_global_alias: Option<DeployApp>,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct GetDeployAppAndVersionVars {
        pub name: String,
        pub owner: String,
        pub version: String,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetDeployAppAndVersionVars")]
    pub struct GetDeployAppAndVersion {
        #[arguments(owner: $owner, name: $name)]
        pub get_deploy_app: Option<DeployApp>,
        #[arguments(owner: $owner, name: $name, version: $version)]
        pub get_deploy_app_version: Option<DeployAppVersion>,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct GetDeployAppVersionVars {
        pub name: String,
        pub owner: String,
        pub version: String,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetDeployAppVersionVars")]
    pub struct GetDeployAppVersion {
        #[arguments(owner: $owner, name: $name, version: $version)]
        pub get_deploy_app_version: Option<DeployAppVersion>,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct CreateNamespaceVars {
        pub name: String,
        pub description: Option<String>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Mutation", variables = "CreateNamespaceVars")]
    pub struct CreateNamespace {
        #[arguments(input: {name: $name, description: $description})]
        pub create_namespace: Option<CreateNamespacePayload>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct CreateNamespacePayload {
        pub namespace: Namespace,
    }

    #[derive(cynic::InputObject, Debug)]
    pub struct CreateNamespaceInput {
        pub name: String,
        pub display_name: Option<String>,
        pub description: Option<String>,
        pub avatar: Option<String>,
        pub client_mutation_id: Option<String>,
    }

    #[derive(cynic::QueryFragment, Debug, Clone)]
    pub struct NamespaceEdge {
        pub node: Option<Namespace>,
    }

    #[derive(cynic::QueryFragment, Debug, Clone)]
    pub struct NamespaceConnection {
        pub edges: Vec<Option<NamespaceEdge>>,
    }

    #[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
    pub struct Namespace {
        pub id: cynic::Id,
        pub name: String,
        pub global_name: String,
    }

    #[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
    pub struct DeployApp {
        pub id: cynic::Id,
        pub name: String,
        pub created_at: DateTime,
        pub description: Option<String>,
        pub active_version: DeployAppVersion,
        pub admin_url: String,
        pub owner: Owner,
        pub url: String,
        pub deleted: bool,
        pub aliases: AppAliasConnection,
    }

    #[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
    pub struct AppAliasConnection {
        pub page_info: PageInfo,
        pub edges: Vec<Option<AppAliasEdge>>,
    }

    #[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
    pub struct AppAliasEdge {
        pub node: Option<AppAlias>,
    }

    #[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
    pub struct AppAlias {
        pub name: String,
    }

    #[derive(cynic::QueryVariables, Debug, Clone)]
    pub struct DeleteAppVars {
        pub app_id: cynic::Id,
    }

    #[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
    pub struct DeleteAppPayload {
        pub success: bool,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Mutation", variables = "DeleteAppVars")]
    pub struct DeleteApp {
        #[arguments(input: { id: $app_id })]
        pub delete_app: Option<DeleteAppPayload>,
    }

    #[derive(cynic::Enum, Clone, Copy, Debug)]
    pub enum DeployAppVersionsSortBy {
        Newest,
        Oldest,
    }

    #[derive(cynic::QueryVariables, Debug, Clone)]
    pub struct GetDeployAppVersionsVars {
        pub owner: String,
        pub name: String,

        pub offset: Option<i32>,
        pub before: Option<String>,
        pub after: Option<String>,
        pub first: Option<i32>,
        pub last: Option<i32>,
        pub sort_by: Option<DeployAppVersionsSortBy>,
    }

    #[derive(cynic::QueryFragment, Debug, Clone)]
    #[cynic(graphql_type = "Query", variables = "GetDeployAppVersionsVars")]
    pub struct GetDeployAppVersions {
        #[arguments(owner: $owner, name: $name)]
        pub get_deploy_app: Option<DeployAppVersions>,
    }

    #[derive(cynic::QueryFragment, Debug, Clone)]
    #[cynic(graphql_type = "DeployApp", variables = "GetDeployAppVersionsVars")]
    pub struct DeployAppVersions {
        #[arguments(
            first: $first,
            last: $last,
            before: $before,
            after: $after,
            offset: $offset,
            sortBy: $sort_by
        )]
        pub versions: DeployAppVersionConnection,
    }

    #[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
    #[cynic(graphql_type = "DeployApp")]
    pub struct SparseDeployApp {
        pub id: cynic::Id,
    }

    #[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
    pub struct DeployAppVersion {
        pub id: cynic::Id,
        pub created_at: DateTime,
        pub version: String,
        pub description: Option<String>,
        pub yaml_config: String,
        pub user_yaml_config: String,
        pub config: String,
        pub json_config: String,
        pub url: String,

        pub app: Option<SparseDeployApp>,
    }

    #[derive(cynic::QueryFragment, Debug, Clone)]
    pub struct DeployAppVersionConnection {
        pub page_info: PageInfo,
        pub edges: Vec<Option<DeployAppVersionEdge>>,
    }

    #[derive(cynic::QueryFragment, Debug, Clone)]
    pub struct DeployAppVersionEdge {
        pub node: Option<DeployAppVersion>,
        pub cursor: String,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct DeployAppConnection {
        pub page_info: PageInfo,
        pub edges: Vec<Option<DeployAppEdge>>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct DeployAppEdge {
        pub node: Option<DeployApp>,
        pub cursor: String,
    }

    #[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
    pub struct PageInfo {
        pub has_next_page: bool,
        pub end_cursor: Option<String>,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct GetNamespaceVars {
        pub name: String,
    }

    #[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
    pub struct MarkAppVersionAsActivePayload {
        pub app: DeployApp,
    }

    #[derive(cynic::InputObject, Debug)]
    pub struct MarkAppVersionAsActiveInput {
        pub app_version: cynic::Id,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct MarkAppVersionAsActiveVars {
        pub input: MarkAppVersionAsActiveInput,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Mutation", variables = "MarkAppVersionAsActiveVars")]
    pub struct MarkAppVersionAsActive {
        #[arguments(input: $input)]
        pub mark_app_version_as_active: Option<MarkAppVersionAsActivePayload>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetNamespaceVars")]
    pub struct GetNamespace {
        #[arguments(name: $name)]
        pub get_namespace: Option<Namespace>,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct GetNamespaceAppsVars {
        pub name: String,
        pub after: Option<String>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetNamespaceAppsVars")]
    pub struct GetNamespaceApps {
        #[arguments(name: $name)]
        pub get_namespace: Option<NamespaceWithApps>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Namespace")]
    #[cynic(variables = "GetNamespaceAppsVars")]
    pub struct NamespaceWithApps {
        pub id: cynic::Id,
        pub name: String,
        #[arguments(after: $after)]
        pub apps: DeployAppConnection,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct PublishDeployAppVars {
        pub config: String,
        pub name: cynic::Id,
        pub owner: Option<cynic::Id>,
        pub make_default: Option<bool>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Mutation", variables = "PublishDeployAppVars")]
    pub struct PublishDeployApp {
        #[arguments(input: { config: { yamlConfig: $config }, name: $name, owner: $owner, makeDefault: $make_default })]
        pub publish_deploy_app: Option<PublishDeployAppPayload>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct PublishDeployAppPayload {
        pub deploy_app_version: DeployAppVersion,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct GenerateDeployTokenVars {
        pub app_version_id: String,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Mutation", variables = "GenerateDeployTokenVars")]
    pub struct GenerateDeployToken {
        #[arguments(input: { deployConfigVersionId: $app_version_id })]
        pub generate_deploy_token: Option<GenerateDeployTokenPayload>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct GenerateDeployTokenPayload {
        pub token: String,
    }

    #[derive(cynic::Enum, Clone, Copy, Debug)]
    pub enum LogStream {
        Stdout,
        Stderr,
        Runtime,
    }

    #[derive(cynic::QueryVariables, Debug, Clone)]
    pub struct GetDeployAppLogsVars {
        pub name: String,
        pub owner: String,
        /// The tag associated with a particular app version. Uses the active
        /// version if not provided.
        pub version: Option<String>,
        /// The lower bound for log messages, in nanoseconds since the Unix
        /// epoch.
        pub starting_from: f64,
        /// The upper bound for log messages, in nanoseconds since the Unix
        /// epoch.
        pub until: Option<f64>,
        pub first: Option<i32>,

        pub streams: Option<Vec<LogStream>>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetDeployAppLogsVars")]
    pub struct GetDeployAppLogs {
        #[arguments(name: $name, owner: $owner, version: $version)]
        pub get_deploy_app_version: Option<DeployAppVersionLogs>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "DeployAppVersion", variables = "GetDeployAppLogsVars")]
    pub struct DeployAppVersionLogs {
        #[arguments(startingFrom: $starting_from, until: $until, first: $first)]
        pub logs: LogConnection,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct LogConnection {
        pub edges: Vec<Option<LogEdge>>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct LogEdge {
        pub node: Option<Log>,
    }

    #[derive(cynic::QueryFragment, Debug, serde::Serialize, PartialEq)]
    pub struct Log {
        pub message: String,
        /// When the message was recorded, in nanoseconds since the Unix epoch.
        pub timestamp: f64,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct GenerateDeployConfigTokenVars {
        pub input: String,
    }
    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Mutation", variables = "GenerateDeployConfigTokenVars")]
    pub struct GenerateDeployConfigToken {
        #[arguments(input: { config: $input })]
        pub generate_deploy_config_token: Option<GenerateDeployConfigTokenPayload>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct GenerateDeployConfigTokenPayload {
        pub token: String,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct GetNodeVars {
        pub id: cynic::Id,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetNodeVars")]
    pub struct GetNode {
        #[arguments(id: $id)]
        pub node: Option<Node>,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct GetDeployAppByIdVars {
        pub app_id: cynic::Id,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetDeployAppByIdVars")]
    pub struct GetDeployAppById {
        #[arguments(id: $app_id)]
        #[cynic(rename = "node")]
        pub app: Option<Node>,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct GetDeployAppAndVersionByIdVars {
        pub app_id: cynic::Id,
        pub version_id: cynic::Id,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetDeployAppAndVersionByIdVars")]
    pub struct GetDeployAppAndVersionById {
        #[arguments(id: $app_id)]
        #[cynic(rename = "node")]
        pub app: Option<Node>,
        #[arguments(id: $version_id)]
        #[cynic(rename = "node")]
        pub version: Option<Node>,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct GetDeployAppVersionByIdVars {
        pub version_id: cynic::Id,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetDeployAppVersionByIdVars")]
    pub struct GetDeployAppVersionById {
        #[arguments(id: $version_id)]
        #[cynic(rename = "node")]
        pub version: Option<Node>,
    }

    #[derive(cynic::QueryFragment, Debug, Clone)]
    #[cynic(graphql_type = "TXTRecord")]
    pub struct TxtRecord {
        pub id: cynic::Id,
        pub created_at: DateTime,
        pub updated_at: DateTime,
        pub deleted_at: Option<DateTime>,
        pub name: Option<String>,
        pub ttl: Option<i32>,
        pub data: String,

        pub domain: DnsDomain,
    }

    #[derive(cynic::QueryFragment, Debug, Clone)]
    #[cynic(graphql_type = "SSHFPRecord")]
    pub struct SshfpRecord {
        pub id: cynic::Id,
        pub created_at: DateTime,
        pub updated_at: DateTime,
        pub deleted_at: Option<DateTime>,
        pub name: Option<String>,
        pub ttl: Option<i32>,
        #[cynic(rename = "type")]
        pub type_: DnsmanagerSshFingerprintRecordTypeChoices,
        pub algorithm: DnsmanagerSshFingerprintRecordAlgorithmChoices,
        pub fingerprint: String,

        pub domain: DnsDomain,
    }

    #[derive(cynic::QueryFragment, Debug, Clone)]
    #[cynic(graphql_type = "SRVRecord")]
    pub struct SrvRecord {
        pub id: cynic::Id,
        pub created_at: DateTime,
        pub updated_at: DateTime,
        pub deleted_at: Option<DateTime>,
        pub name: Option<String>,
        pub ttl: Option<i32>,
        pub service: String,
        pub protocol: String,
        pub priority: i32,
        pub weight: i32,
        pub port: i32,
        pub target: String,

        pub domain: DnsDomain,
    }

    #[derive(cynic::QueryFragment, Debug, Clone)]
    #[cynic(graphql_type = "SOARecord")]
    pub struct SoaRecord {
        pub id: cynic::Id,
        pub created_at: DateTime,
        pub updated_at: DateTime,
        pub deleted_at: Option<DateTime>,
        pub name: Option<String>,
        pub ttl: Option<i32>,
        pub mname: String,
        pub rname: String,
        pub serial: BigInt,
        pub refresh: BigInt,
        pub retry: BigInt,
        pub expire: BigInt,
        pub minimum: BigInt,

        pub domain: DnsDomain,
    }

    #[derive(cynic::Enum, Debug, Clone, Copy)]
    pub enum DNSRecordsSortBy {
        Newest,
        Oldest,
    }

    #[derive(cynic::QueryVariables, Debug, Clone)]
    pub struct GetAllDnsRecordsVariables {
        pub after: Option<String>,
        pub updated_after: Option<DateTime>,
        pub sort_by: Option<DNSRecordsSortBy>,
        pub first: Option<i32>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetAllDnsRecordsVariables")]
    pub struct GetAllDnsRecords {
        #[arguments(
            first: $first,
            after: $after,
            updatedAfter: $updated_after,
            sortBy: $sort_by
        )]
        #[cynic(rename = "getAllDNSRecords")]
        pub get_all_dnsrecords: DnsRecordConnection,
    }

    #[derive(cynic::QueryFragment, Debug, Clone)]
    #[cynic(graphql_type = "PTRRecord")]
    pub struct PtrRecord {
        pub id: cynic::Id,
        pub created_at: DateTime,
        pub updated_at: DateTime,
        pub deleted_at: Option<DateTime>,
        pub name: Option<String>,
        pub ttl: Option<i32>,
        pub ptrdname: String,

        pub domain: DnsDomain,
    }

    #[derive(cynic::QueryFragment, Debug, Clone)]
    #[cynic(graphql_type = "NSRecord")]
    pub struct NsRecord {
        pub id: cynic::Id,
        pub created_at: DateTime,
        pub updated_at: DateTime,
        pub deleted_at: Option<DateTime>,
        pub name: Option<String>,
        pub ttl: Option<i32>,
        pub nsdname: String,

        pub domain: DnsDomain,
    }

    #[derive(cynic::QueryFragment, Debug, Clone)]
    #[cynic(graphql_type = "MXRecord")]
    pub struct MxRecord {
        pub id: cynic::Id,
        pub created_at: DateTime,
        pub updated_at: DateTime,
        pub deleted_at: Option<DateTime>,
        pub name: Option<String>,
        pub ttl: Option<i32>,
        pub preference: i32,
        pub exchange: String,

        pub domain: DnsDomain,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "DNSRecordConnection")]
    pub struct DnsRecordConnection {
        pub page_info: PageInfo,
        pub edges: Vec<Option<DnsRecordEdge>>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "DNSRecordEdge")]
    pub struct DnsRecordEdge {
        pub node: Option<DnsRecord>,
    }

    #[derive(cynic::QueryFragment, Debug, Clone)]
    #[cynic(graphql_type = "DNAMERecord")]
    pub struct DNameRecord {
        pub id: cynic::Id,
        pub created_at: DateTime,
        pub updated_at: DateTime,
        pub deleted_at: Option<DateTime>,
        pub name: Option<String>,
        pub ttl: Option<i32>,
        pub d_name: String,

        pub domain: DnsDomain,
    }

    #[derive(cynic::QueryFragment, Debug, Clone)]
    #[cynic(graphql_type = "CNAMERecord")]
    pub struct CNameRecord {
        pub id: cynic::Id,
        pub created_at: DateTime,
        pub updated_at: DateTime,
        pub deleted_at: Option<DateTime>,
        pub name: Option<String>,
        pub ttl: Option<i32>,
        pub c_name: String,

        pub domain: DnsDomain,
    }

    #[derive(cynic::QueryFragment, Debug, Clone)]
    #[cynic(graphql_type = "CAARecord")]
    pub struct CaaRecord {
        pub id: cynic::Id,
        pub created_at: DateTime,
        pub updated_at: DateTime,
        pub deleted_at: Option<DateTime>,
        pub name: Option<String>,
        pub ttl: Option<i32>,
        pub value: String,
        pub flags: i32,
        pub tag: DnsmanagerCertificationAuthorityAuthorizationRecordTagChoices,

        pub domain: DnsDomain,
    }

    #[derive(cynic::QueryFragment, Debug, Clone)]
    #[cynic(graphql_type = "ARecord")]
    pub struct ARecord {
        pub id: cynic::Id,
        pub created_at: DateTime,
        pub updated_at: DateTime,
        pub deleted_at: Option<DateTime>,
        pub name: Option<String>,
        pub ttl: Option<i32>,
        pub address: String,

        pub domain: DnsDomain,
    }

    #[derive(cynic::QueryFragment, Debug, Clone)]
    #[cynic(graphql_type = "AAAARecord")]
    pub struct AaaaRecord {
        pub id: cynic::Id,
        pub created_at: DateTime,
        pub updated_at: DateTime,
        pub deleted_at: Option<DateTime>,
        pub name: Option<String>,
        pub ttl: Option<i32>,
        pub address: String,

        pub domain: DnsDomain,
    }

    #[derive(cynic::InlineFragments, Debug, Clone)]
    #[cynic(graphql_type = "DNSRecord")]
    pub enum DnsRecord {
        A(ARecord),
        AAAA(AaaaRecord),
        CName(CNameRecord),
        Txt(TxtRecord),
        Mx(MxRecord),
        Ns(NsRecord),
        CAA(CaaRecord),
        DName(DNameRecord),
        Ptr(PtrRecord),
        Soa(SoaRecord),
        Srv(SrvRecord),
        Sshfp(SshfpRecord),
        #[cynic(fallback)]
        Unknown,
    }

    impl DnsRecord {
        pub fn id(&self) -> &str {
            match self {
                DnsRecord::A(record) => record.id.inner(),
                DnsRecord::AAAA(record) => record.id.inner(),
                DnsRecord::CName(record) => record.id.inner(),
                DnsRecord::Txt(record) => record.id.inner(),
                DnsRecord::Mx(record) => record.id.inner(),
                DnsRecord::Ns(record) => record.id.inner(),
                DnsRecord::CAA(record) => record.id.inner(),
                DnsRecord::DName(record) => record.id.inner(),
                DnsRecord::Ptr(record) => record.id.inner(),
                DnsRecord::Soa(record) => record.id.inner(),
                DnsRecord::Srv(record) => record.id.inner(),
                DnsRecord::Sshfp(record) => record.id.inner(),
                DnsRecord::Unknown => "",
            }
        }

        pub fn domain(&self) -> Option<&DnsDomain> {
            match self {
                DnsRecord::A(record) => Some(&record.domain),
                DnsRecord::AAAA(record) => Some(&record.domain),
                DnsRecord::CName(record) => Some(&record.domain),
                DnsRecord::Txt(record) => Some(&record.domain),
                DnsRecord::Mx(record) => Some(&record.domain),
                DnsRecord::Ns(record) => Some(&record.domain),
                DnsRecord::CAA(record) => Some(&record.domain),
                DnsRecord::DName(record) => Some(&record.domain),
                DnsRecord::Ptr(record) => Some(&record.domain),
                DnsRecord::Soa(record) => Some(&record.domain),
                DnsRecord::Srv(record) => Some(&record.domain),
                DnsRecord::Sshfp(record) => Some(&record.domain),
                DnsRecord::Unknown => None,
            }
        }
    }

    #[derive(cynic::Enum, Clone, Copy, Debug)]
    pub enum DnsmanagerCertificationAuthorityAuthorizationRecordTagChoices {
        Issue,
        Issuewild,
        Iodef,
    }

    impl DnsmanagerCertificationAuthorityAuthorizationRecordTagChoices {
        pub fn as_str(self) -> &'static str {
            match self {
                Self::Issue => "issue",
                Self::Issuewild => "issuewild",
                Self::Iodef => "iodef",
            }
        }
    }

    #[derive(cynic::Enum, Clone, Copy, Debug)]
    pub enum DnsmanagerSshFingerprintRecordAlgorithmChoices {
        #[cynic(rename = "A_1")]
        A1,
        #[cynic(rename = "A_2")]
        A2,
        #[cynic(rename = "A_3")]
        A3,
        #[cynic(rename = "A_4")]
        A4,
    }

    #[derive(cynic::Enum, Clone, Copy, Debug)]
    pub enum DnsmanagerSshFingerprintRecordTypeChoices {
        #[cynic(rename = "A_1")]
        A1,
        #[cynic(rename = "A_2")]
        A2,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct GetDomainVars {
        pub domain: String,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetDomainVars")]
    pub struct GetDomain {
        #[arguments(name: $domain)]
        pub get_domain: Option<DnsDomain>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetDomainVars")]
    pub struct GetDomainWithRecords {
        #[arguments(name: $domain)]
        pub get_domain: Option<DnsDomainWithRecords>,
    }

    #[derive(cynic::QueryFragment, Debug, Clone)]
    #[cynic(graphql_type = "DNSDomain")]
    pub struct DnsDomain {
        pub id: cynic::Id,
        pub name: String,
        pub slug: String,
    }

    #[derive(cynic::QueryFragment, Debug, Clone)]
    #[cynic(graphql_type = "DNSDomain")]
    pub struct DnsDomainWithRecords {
        pub id: cynic::Id,
        pub name: String,
        pub slug: String,
        pub records: Option<Vec<Option<DnsRecord>>>,
    }

    #[derive(cynic::Scalar, Debug, Clone)]
    pub struct BigInt(pub String);

    #[derive(cynic::InlineFragments, Debug)]
    pub enum Node {
        DeployApp(Box<DeployApp>),
        DeployAppVersion(Box<DeployAppVersion>),
        #[cynic(fallback)]
        Unknown,
    }

    impl Node {
        pub fn into_deploy_app(self) -> Option<DeployApp> {
            match self {
                Node::DeployApp(app) => Some(*app),
                _ => None,
            }
        }

        pub fn into_deploy_app_version(self) -> Option<DeployAppVersion> {
            match self {
                Node::DeployAppVersion(version) => Some(*version),
                _ => None,
            }
        }
    }
}

#[allow(non_snake_case, non_camel_case_types)]
mod schema {
    cynic::use_schema!(r#"schema.graphql"#);
}
