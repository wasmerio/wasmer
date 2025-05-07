pub use queries::*;

pub use cynic::Id;

#[cynic::schema_for_derives(file = r#"schema.graphql"#, module = "schema")]
mod queries {
    use serde::Serialize;
    use time::OffsetDateTime;

    use super::schema;

    #[derive(cynic::Scalar, Debug, Clone, PartialEq, Eq)]
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
    pub struct ViewerCanVariables<'a> {
        pub action: OwnerAction,
        pub owner_name: &'a str,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "ViewerCanVariables")]
    pub struct ViewerCan {
        #[arguments(action: $action, ownerName: $owner_name)]
        pub viewer_can: bool,
    }

    #[derive(cynic::Enum, Clone, Copy, Debug)]
    pub enum OwnerAction {
        DeployApp,
        PublishPackage,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct RevokeTokenVariables {
        pub token: String,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Mutation", variables = "RevokeTokenVariables")]
    pub struct RevokeToken {
        #[arguments(input: { token: $token })]
        pub revoke_api_token: Option<RevokeAPITokenPayload>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct RevokeAPITokenPayload {
        pub success: Option<bool>,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct CreateNewNonceVariables {
        pub callback_url: String,
        pub name: String,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Mutation", variables = "CreateNewNonceVariables")]
    pub struct CreateNewNonce {
        #[arguments(input: { callbackUrl: $callback_url, name: $name })]
        pub new_nonce: Option<NewNoncePayload>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct NewNoncePayload {
        pub client_mutation_id: Option<String>,
        pub nonce: Nonce,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct Nonce {
        pub auth_url: String,
        pub callback_url: String,
        pub created_at: DateTime,
        pub expired: bool,
        pub id: cynic::Id,
        pub is_validated: bool,
        pub name: String,
        pub secret: String,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query")]
    pub struct GetCurrentUser {
        pub viewer: Option<User>,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct GetCurrentUserWithNamespacesVars {
        pub namespace_role: Option<GrapheneRole>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetCurrentUserWithNamespacesVars")]
    pub struct GetCurrentUserWithNamespaces {
        pub viewer: Option<UserWithNamespaces>,
    }

    #[derive(cynic::QueryFragment, Debug, serde::Serialize)]
    pub struct User {
        pub id: cynic::Id,
        pub username: String,
    }

    #[derive(cynic::QueryFragment, Debug, Clone, Serialize)]
    pub struct Package {
        pub id: cynic::Id,
        pub package_name: String,
        pub namespace: Option<String>,
        pub last_version: Option<PackageVersion>,
        pub private: bool,
    }

    #[derive(cynic::QueryFragment, Debug, Clone, Serialize)]
    pub struct PackageDistribution {
        pub pirita_sha256_hash: Option<String>,
        pub pirita_download_url: Option<String>,
        pub download_url: Option<String>,
        pub size: Option<i32>,
        pub pirita_size: Option<i32>,
        pub webc_version: Option<WebcVersion>,
        pub webc_manifest: Option<JSONString>,
    }

    #[derive(cynic::Enum, Clone, Copy, Debug)]
    pub enum WebcVersion {
        V2,
        V3,
    }

    #[derive(cynic::QueryFragment, Debug, Clone, Serialize)]
    pub struct WebcImage {
        pub created_at: DateTime,
        pub updated_at: DateTime,
        pub webc_url: String,
        pub webc_sha256: String,
        pub file_size: BigInt,
        pub manifest: JSONString,
        pub version: Option<WebcVersion>,
    }

    #[derive(cynic::QueryFragment, Debug, Clone, Serialize)]
    pub struct PackageWebc {
        pub id: cynic::Id,
        pub created_at: DateTime,
        pub updated_at: DateTime,
        pub tag: String,
        pub is_archived: bool,
        pub webc_url: String,
        pub webc: Option<WebcImage>,
        pub webc_v3: Option<WebcImage>,
    }

    #[derive(cynic::QueryFragment, Debug, Clone, Serialize)]
    pub struct PackageVersion {
        pub id: cynic::Id,
        pub version: String,
        pub created_at: DateTime,
        pub distribution: PackageDistribution,
    }

    #[derive(cynic::QueryFragment, Debug, Clone, Serialize)]
    #[cynic(graphql_type = "PackageVersion")]
    pub struct PackageVersionWithPackage {
        pub id: cynic::Id,
        pub version: String,
        pub created_at: DateTime,
        pub pirita_manifest: Option<JSONString>,
        pub package: Package,

        #[arguments(version: "V3")]
        #[cynic(rename = "distribution")]
        pub distribution_v3: PackageDistribution,

        #[arguments(version: "V2")]
        #[cynic(rename = "distribution")]
        pub distribution_v2: PackageDistribution,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct GetAppTemplateFromSlugVariables {
        pub slug: String,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetAppTemplateFromSlugVariables")]
    pub struct GetAppTemplateFromSlug {
        #[arguments(slug: $slug)]
        pub get_app_template: Option<AppTemplate>,
    }

    #[derive(cynic::Enum, Clone, Copy, Debug)]
    pub enum AppTemplatesSortBy {
        Newest,
        Oldest,
        Popular,
    }

    #[derive(cynic::QueryVariables, Debug, Clone)]
    pub struct GetAppTemplatesFromFrameworkVars {
        pub framework_slug: String,
        pub first: i32,
        pub after: Option<String>,
        pub sort_by: Option<AppTemplatesSortBy>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetAppTemplatesFromFrameworkVars")]
    pub struct GetAppTemplatesFromFramework {
        #[arguments(
            frameworkSlug: $framework_slug,
            first: $first,
            after: $after,
            sortBy: $sort_by
        )]
        pub get_app_templates: Option<AppTemplateConnection>,
    }

    #[derive(cynic::QueryVariables, Debug, Clone)]
    pub struct GetAppTemplatesFromLanguageVars {
        pub language_slug: String,
        pub first: i32,
        pub after: Option<String>,
        pub sort_by: Option<AppTemplatesSortBy>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetAppTemplatesFromLanguageVars")]
    pub struct GetAppTemplatesFromLanguage {
        #[arguments(
            languageSlug: $language_slug,
            first: $first,
            after: $after,
            sortBy: $sort_by
        )]
        pub get_app_templates: Option<AppTemplateConnection>,
    }

    #[derive(cynic::QueryVariables, Debug, Clone)]
    pub struct GetAppTemplatesVars {
        pub category_slug: String,
        pub first: i32,
        pub after: Option<String>,
        pub sort_by: Option<AppTemplatesSortBy>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetAppTemplatesVars")]
    pub struct GetAppTemplates {
        #[arguments(
            categorySlug: $category_slug,
            first: $first,
            after: $after,
            sortBy: $sort_by
        )]
        pub get_app_templates: Option<AppTemplateConnection>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct AppTemplateConnection {
        pub edges: Vec<Option<AppTemplateEdge>>,
        pub page_info: PageInfo,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct AppTemplateEdge {
        pub node: Option<AppTemplate>,
        pub cursor: String,
    }

    #[derive(serde::Serialize, cynic::QueryFragment, PartialEq, Eq, Debug)]
    pub struct AppTemplate {
        #[serde(rename = "demoUrl")]
        pub demo_url: String,
        pub language: String,
        pub name: String,
        pub framework: String,
        #[serde(rename = "createdAt")]
        pub created_at: DateTime,
        pub description: String,
        pub id: cynic::Id,
        #[serde(rename = "isPublic")]
        pub is_public: bool,
        #[serde(rename = "repoLicense")]
        pub repo_license: String,
        pub readme: String,
        #[serde(rename = "repoUrl")]
        pub repo_url: String,
        pub slug: String,
        #[serde(rename = "updatedAt")]
        pub updated_at: DateTime,
        #[serde(rename = "useCases")]
        pub use_cases: Jsonstring,
    }

    #[derive(cynic::QueryVariables, Debug, Clone)]
    pub struct GetTemplateFrameworksVars {
        pub after: Option<String>,
        pub first: Option<i32>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetTemplateFrameworksVars")]
    pub struct GetTemplateFrameworks {
        #[arguments(after: $after, first: $first)]
        pub get_template_frameworks: Option<TemplateFrameworkConnection>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct TemplateFrameworkConnection {
        pub edges: Vec<Option<TemplateFrameworkEdge>>,
        pub page_info: PageInfo,
        pub total_count: Option<i32>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct TemplateFrameworkEdge {
        pub cursor: String,
        pub node: Option<TemplateFramework>,
    }

    #[derive(serde::Serialize, cynic::QueryFragment, PartialEq, Eq, Debug)]
    pub struct TemplateFramework {
        #[serde(rename = "createdAt")]
        pub created_at: DateTime,
        pub id: cynic::Id,
        pub name: String,
        pub slug: String,
        #[serde(rename = "updatedAt")]
        pub updated_at: DateTime,
    }

    #[derive(cynic::QueryVariables, Debug, Clone)]
    pub struct GetTemplateLanguagesVars {
        pub after: Option<String>,
        pub first: Option<i32>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetTemplateLanguagesVars")]
    pub struct GetTemplateLanguages {
        #[arguments(after: $after, first: $first)]
        pub get_template_languages: Option<TemplateLanguageConnection>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct TemplateLanguageConnection {
        pub edges: Vec<Option<TemplateLanguageEdge>>,
        pub page_info: PageInfo,
        pub total_count: Option<i32>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct TemplateLanguageEdge {
        pub cursor: String,
        pub node: Option<TemplateLanguage>,
    }

    #[derive(serde::Serialize, cynic::QueryFragment, PartialEq, Eq, Debug)]
    pub struct TemplateLanguage {
        #[serde(rename = "createdAt")]
        pub created_at: DateTime,
        pub id: cynic::Id,
        pub name: String,
        pub slug: String,
        #[serde(rename = "updatedAt")]
        pub updated_at: DateTime,
    }

    #[derive(cynic::Scalar, Debug, Clone, PartialEq, Eq)]
    #[cynic(graphql_type = "JSONString")]
    pub struct Jsonstring(pub String);

    #[derive(cynic::QueryVariables, Debug)]
    pub struct GetPackageReleaseVars {
        pub hash: String,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetPackageReleaseVars")]
    pub struct GetPackageRelease {
        #[arguments(hash: $hash)]
        pub get_package_release: Option<PackageWebc>,
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

    #[derive(cynic::QueryVariables, Debug)]
    pub struct PushPackageReleaseVariables<'a> {
        pub name: Option<&'a str>,
        pub namespace: &'a str,
        pub private: Option<bool>,
        pub signed_url: &'a str,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Mutation", variables = "PushPackageReleaseVariables")]
    pub struct PushPackageRelease {
        #[arguments(input: { name: $name, namespace: $namespace, private: $private, signedUrl: $signed_url })]
        pub push_package_release: Option<PushPackageReleasePayload>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct PushPackageReleasePayload {
        pub package_webc: Option<PackageWebc>,
        pub success: bool,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct TagPackageReleaseVariables<'a> {
        pub description: Option<&'a str>,
        pub homepage: Option<&'a str>,
        pub license: Option<&'a str>,
        pub license_file: Option<&'a str>,
        pub manifest: Option<&'a str>,
        pub name: &'a str,
        pub namespace: Option<&'a str>,
        pub package_release_id: &'a cynic::Id,
        pub private: Option<bool>,
        pub readme: Option<&'a str>,
        pub repository: Option<&'a str>,
        pub version: &'a str,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Mutation", variables = "TagPackageReleaseVariables")]
    pub struct TagPackageRelease {
        #[arguments(input: { description: $description, homepage: $homepage, license: $license, licenseFile: $license_file, manifest: $manifest, name: $name, namespace: $namespace, packageReleaseId: $package_release_id, private: $private, readme: $readme, repository: $repository, version: $version })]
        pub tag_package_release: Option<TagPackageReleasePayload>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct TagPackageReleasePayload {
        pub success: bool,
        pub package_version: Option<PackageVersion>,
    }

    #[derive(cynic::InputObject, Debug)]
    pub struct InputSignature<'a> {
        pub public_key_key_id: &'a str,
        pub data: &'a str,
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

    #[derive(cynic::QueryVariables, Debug, Clone, Default)]
    pub struct AllPackageReleasesVars {
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
    #[cynic(graphql_type = "Query", variables = "AllPackageReleasesVars")]
    pub struct GetAllPackageReleases {
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
        pub all_package_releases: PackageWebcConnection,
    }

    impl GetAllPackageReleases {
        pub fn into_packages(self) -> Vec<PackageWebc> {
            self.all_package_releases
                .edges
                .into_iter()
                .flatten()
                .filter_map(|x| x.node)
                .collect()
        }
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct GetSignedUrlForPackageUploadVariables<'a> {
        pub expires_after_seconds: Option<i32>,
        pub filename: Option<&'a str>,
        pub name: Option<&'a str>,
        pub version: Option<&'a str>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(
        graphql_type = "Query",
        variables = "GetSignedUrlForPackageUploadVariables"
    )]
    pub struct GetSignedUrlForPackageUpload {
        #[arguments(name: $name, version: $version, filename: $filename, expiresAfterSeconds: $expires_after_seconds)]
        pub get_signed_url_for_package_upload: Option<SignedUrl>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct SignedUrl {
        pub url: String,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct PackageWebcConnection {
        pub page_info: PageInfo,
        pub edges: Vec<Option<PackageWebcEdge>>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct PackageWebcEdge {
        pub node: Option<PackageWebc>,
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
        pub sort: Option<DeployAppsSortBy>,
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
        #[arguments(after: $after, sortBy: $sort)]
        pub apps: DeployAppConnection,
    }

    #[derive(cynic::QueryFragment, Serialize, Debug, Clone)]
    pub struct Owner {
        pub global_name: String,
    }

    #[derive(cynic::QueryFragment, Debug, Clone, Serialize)]
    #[cynic(graphql_type = "User", variables = "GetCurrentUserWithNamespacesVars")]
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

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetDeployAppVars")]
    pub struct GetDeployAppS3Credentials {
        #[arguments(owner: $owner, name: $name)]
        pub get_deploy_app: Option<AppWithS3Credentials>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "DeployApp", variables = "GetDeployAppVars")]
    pub struct AppWithS3Credentials {
        pub s3_credentials: Option<S3Credentials>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct S3Credentials {
        pub access_key: String,
        pub secret_key: String,
        pub endpoint: String,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct RotateS3SecretsForAppVariables {
        pub id: cynic::Id,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(
        graphql_type = "Mutation",
        variables = "RotateS3SecretsForAppVariables"
    )]
    pub struct RotateS3SecretsForApp {
        #[arguments(input: { id: $id })]
        pub rotate_s3_secrets_for_app: Option<RotateS3SecretsForAppPayload>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct RotateS3SecretsForAppPayload {
        pub client_mutation_id: Option<String>,
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
    pub(crate) struct GetAppVolumesVars {
        pub name: String,
        pub owner: String,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetAppVolumesVars")]
    pub(crate) struct GetAppVolumes {
        #[arguments(owner: $owner, name: $name)]
        pub get_deploy_app: Option<AppVolumes>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "DeployApp")]
    pub(crate) struct AppVolumes {
        pub active_version: Option<AppVersionVolumes>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "DeployAppVersion")]
    pub(crate) struct AppVersionVolumes {
        pub volumes: Option<Vec<Option<AppVersionVolume>>>,
    }

    #[derive(serde::Serialize, cynic::QueryFragment, Debug)]
    pub struct AppVersionVolume {
        pub name: String,
        pub size: Option<BigInt>,
        pub used_size: Option<BigInt>,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub(crate) struct GetAppDatabasesVars {
        pub name: String,
        pub owner: String,
        pub after: Option<String>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetAppDatabasesVars")]
    pub(crate) struct GetAppDatabases {
        #[arguments(owner: $owner, name: $name)]
        pub get_deploy_app: Option<AppDatabases>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub(crate) struct AppDatabaseConnection {
        pub page_info: PageInfo,
        pub edges: Vec<Option<AppDatabaseEdge>>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "DeployApp")]
    pub(crate) struct AppDatabases {
        pub databases: AppDatabaseConnection,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub(crate) struct AppDatabaseEdge {
        pub node: Option<AppDatabase>,
    }

    #[derive(serde::Serialize, cynic::QueryFragment, Debug)]
    pub struct AppDatabase {
        pub id: cynic::Id,
        pub name: String,
        pub created_at: DateTime,
        pub updated_at: DateTime,
        pub deleted_at: Option<DateTime>,
        pub username: String,
        pub db_explorer_url: Option<String>,
        pub host: String,
        pub port: String,
        pub password: Option<String>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct RegisterDomainPayload {
        pub success: bool,
        pub domain: Option<DnsDomain>,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct RegisterDomainVars {
        pub name: String,
        pub namespace: Option<String>,
        pub import_records: Option<bool>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Mutation", variables = "RegisterDomainVars")]
    pub struct RegisterDomain {
        #[arguments(input: {name: $name, importRecords: $import_records, namespace: $namespace})]
        pub register_domain: Option<RegisterDomainPayload>,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct UpsertDomainFromZoneFileVars {
        pub zone_file: String,
        pub delete_missing_records: Option<bool>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Mutation", variables = "UpsertDomainFromZoneFileVars")]
    pub struct UpsertDomainFromZoneFile {
        #[arguments(input: {zoneFile: $zone_file, deleteMissingRecords: $delete_missing_records})]
        pub upsert_domain_from_zone_file: Option<UpsertDomainFromZoneFilePayload>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct UpsertDomainFromZoneFilePayload {
        pub success: bool,
        pub domain: DnsDomain,
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

    #[derive(cynic::QueryFragment, Debug, Clone, Serialize)]
    pub struct NamespaceEdge {
        pub node: Option<Namespace>,
    }

    #[derive(cynic::QueryFragment, Debug, Clone, Serialize)]
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
        pub updated_at: DateTime,
        pub description: Option<String>,
        pub active_version: Option<DeployAppVersion>,
        pub admin_url: String,
        pub owner: Owner,
        pub url: String,
        pub permalink: String,
        pub deleted: bool,
        pub aliases: AppAliasConnection,
        pub s3_url: Option<Url>,
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
        pub hostname: String,
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

    #[derive(cynic::QueryFragment, Debug, Clone, Serialize)]
    #[cynic(graphql_type = "Query", variables = "GetDeployAppVersionsVars")]
    pub struct GetDeployAppVersions {
        #[arguments(owner: $owner, name: $name)]
        pub get_deploy_app: Option<DeployAppVersions>,
    }

    #[derive(cynic::QueryFragment, Debug, Clone, Serialize)]
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

    #[derive(cynic::QueryVariables, Debug, Clone)]
    pub struct GetDeployAppVersionsByIdVars {
        pub id: cynic::Id,

        pub offset: Option<i32>,
        pub before: Option<String>,
        pub after: Option<String>,
        pub first: Option<i32>,
        pub last: Option<i32>,
        pub sort_by: Option<DeployAppVersionsSortBy>,
    }

    #[derive(cynic::QueryFragment, Debug, Clone, Serialize)]
    #[cynic(graphql_type = "DeployApp", variables = "GetDeployAppVersionsByIdVars")]
    pub struct DeployAppVersionsById {
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

    #[derive(cynic::QueryFragment, Debug, Clone)]
    #[cynic(graphql_type = "Query", variables = "GetDeployAppVersionsByIdVars")]
    pub struct GetDeployAppVersionsById {
        #[arguments(id: $id)]
        pub node: Option<NodeDeployAppVersions>,
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
        pub updated_at: DateTime,
        pub version: String,
        pub description: Option<String>,
        pub yaml_config: String,
        pub user_yaml_config: String,
        pub config: String,
        pub json_config: String,
        pub url: String,
        pub disabled_at: Option<DateTime>,
        pub disabled_reason: Option<String>,

        pub app: Option<SparseDeployApp>,
    }

    #[derive(cynic::QueryFragment, Debug, Clone, Serialize)]
    pub struct DeployAppVersionConnection {
        pub page_info: PageInfo,
        pub edges: Vec<Option<DeployAppVersionEdge>>,
    }

    #[derive(cynic::QueryFragment, Debug, Clone, Serialize)]
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
        pub sort: Option<DeployAppsSortBy>,
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
        #[arguments(after: $after, sortBy: $sort)]
        pub apps: DeployAppConnection,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct RedeployActiveAppVariables {
        pub id: cynic::Id,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Mutation", variables = "RedeployActiveAppVariables")]
    pub struct RedeployActiveApp {
        #[arguments(input: { id: $id })]
        pub redeploy_active_version: Option<RedeployActiveVersionPayload>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct RedeployActiveVersionPayload {
        pub app: DeployApp,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct GetAppDeploymentsVariables {
        pub after: Option<String>,
        pub first: Option<i32>,
        pub name: String,
        pub offset: Option<i32>,
        pub owner: String,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetAppDeploymentsVariables")]
    pub struct GetAppDeployments {
        #[arguments(owner: $owner, name: $name)]
        pub get_deploy_app: Option<DeployAppDeployments>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "DeployApp", variables = "GetAppDeploymentsVariables")]
    pub struct DeployAppDeployments {
        // FIXME: add $offset, $after, currently causes an error from the backend
        // #[arguments(first: $first, after: $after, offset: $offset)]
        pub deployments: Option<DeploymentConnection>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct DeploymentConnection {
        pub page_info: PageInfo,
        pub edges: Vec<Option<DeploymentEdge>>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct DeploymentEdge {
        pub node: Option<Deployment>,
    }

    #[allow(clippy::large_enum_variant)]
    #[derive(cynic::InlineFragments, Debug, Clone, Serialize)]
    pub enum Deployment {
        AutobuildRepository(AutobuildRepository),
        NakedDeployment(NakedDeployment),
        #[cynic(fallback)]
        Other,
    }

    #[derive(cynic::QueryFragment, serde::Serialize, Debug, Clone)]
    pub struct NakedDeployment {
        pub id: cynic::Id,
        pub created_at: DateTime,
        pub updated_at: DateTime,
        pub app_version: Option<DeployAppVersion>,
    }

    #[derive(cynic::QueryFragment, serde::Serialize, Debug, Clone)]
    pub struct AutobuildRepository {
        pub id: cynic::Id,
        pub build_id: Uuid,
        pub created_at: DateTime,
        pub updated_at: DateTime,
        pub status: StatusEnum,
        pub log_url: Option<String>,
        pub repo_url: String,
    }

    #[derive(cynic::Enum, Clone, Copy, Debug)]
    pub enum StatusEnum {
        Success,
        Working,
        Failure,
        Queued,
        Timeout,
        InternalError,
        Cancelled,
        Running,
    }

    impl StatusEnum {
        pub fn as_str(&self) -> &'static str {
            match self {
                Self::Success => "success",
                Self::Working => "working",
                Self::Failure => "failure",
                Self::Queued => "queued",
                Self::Timeout => "timeout",
                Self::InternalError => "internal_error",
                Self::Cancelled => "cancelled",
                Self::Running => "running",
            }
        }
    }

    #[derive(cynic::Scalar, Debug, Clone)]
    #[cynic(graphql_type = "UUID")]
    pub struct Uuid(pub String);

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

    #[derive(cynic::Enum, Clone, Copy, Debug, PartialEq)]
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

        pub request_id: Option<String>,

        pub instance_ids: Option<Vec<String>>,

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
        #[arguments(startingFrom: $starting_from, until: $until, first: $first, instanceIds: $instance_ids, requestId: $request_id, streams: $streams)]
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
        pub stream: Option<LogStream>,
        pub instance_id: String,
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

    #[derive(cynic::QueryVariables, Debug)]
    pub struct DeleteAppSecretVariables {
        pub id: cynic::Id,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Mutation", variables = "DeleteAppSecretVariables")]
    pub struct DeleteAppSecret {
        #[arguments(input: { id: $id })]
        pub delete_app_secret: Option<DeleteAppSecretPayload>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct DeleteAppSecretPayload {
        pub success: bool,
    }
    #[derive(cynic::QueryVariables, Debug, Clone)]
    pub struct GetAllAppSecretsVariables {
        pub after: Option<String>,
        pub app_id: cynic::Id,
        pub before: Option<String>,
        pub first: Option<i32>,
        pub last: Option<i32>,
        pub offset: Option<i32>,
        pub names: Option<Vec<String>>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetAllAppSecretsVariables")]
    pub struct GetAllAppSecrets {
        #[arguments(appId: $app_id, after: $after, before: $before, first: $first, last: $last, offset: $offset, names: $names)]
        pub get_app_secrets: Option<SecretConnection>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct SecretConnection {
        pub edges: Vec<Option<SecretEdge>>,
        pub page_info: PageInfo,
        pub total_count: Option<i32>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct SecretEdge {
        pub cursor: String,
        pub node: Option<Secret>,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct GetAppSecretVariables {
        pub app_id: cynic::Id,
        pub secret_name: String,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetAppSecretVariables")]
    pub struct GetAppSecret {
        #[arguments(appId: $app_id, secretName: $secret_name)]
        pub get_app_secret: Option<Secret>,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct GetAppSecretValueVariables {
        pub id: cynic::Id,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetAppSecretValueVariables")]
    pub struct GetAppSecretValue {
        #[arguments(id: $id)]
        pub get_secret_value: Option<String>,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct UpsertAppSecretVariables<'a> {
        pub app_id: cynic::Id,
        pub name: &'a str,
        pub value: &'a str,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Mutation", variables = "UpsertAppSecretVariables")]
    pub struct UpsertAppSecret {
        #[arguments(input: { appId: $app_id, name: $name, value: $value })]
        pub upsert_app_secret: Option<UpsertAppSecretPayload>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct UpsertAppSecretPayload {
        pub secret: Secret,
        pub success: bool,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct UpsertAppSecretsVariables {
        pub app_id: cynic::Id,
        pub secrets: Option<Vec<SecretInput>>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Mutation", variables = "UpsertAppSecretsVariables")]
    pub struct UpsertAppSecrets {
        #[arguments(input: { appId: $app_id, secrets: $secrets })]
        pub upsert_app_secrets: Option<UpsertAppSecretsPayload>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct UpsertAppSecretsPayload {
        pub secrets: Vec<Option<Secret>>,
        pub success: bool,
    }

    #[derive(cynic::InputObject, Debug)]
    pub struct SecretInput {
        pub name: String,
        pub value: String,
    }
    #[derive(cynic::QueryFragment, Debug, Serialize)]
    pub struct Secret {
        #[serde(skip_serializing)]
        pub id: cynic::Id,
        pub name: String,
        pub created_at: DateTime,
        pub updated_at: DateTime,
    }

    #[derive(cynic::QueryVariables, Debug, Clone)]
    pub struct GetAllAppRegionsVariables {
        pub after: Option<String>,
        pub before: Option<String>,
        pub first: Option<i32>,
        pub last: Option<i32>,
        pub offset: Option<i32>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetAllAppRegionsVariables")]
    pub struct GetAllAppRegions {
        #[arguments(after: $after, offset: $offset, before: $before, first: $first, last: $last)]
        pub get_app_regions: AppRegionConnection,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct AppRegionConnection {
        pub edges: Vec<Option<AppRegionEdge>>,
        pub page_info: PageInfo,
        pub total_count: Option<i32>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct AppRegionEdge {
        pub cursor: String,
        pub node: Option<AppRegion>,
    }

    #[derive(cynic::QueryFragment, Debug, Serialize)]
    pub struct AppRegion {
        pub city: String,
        pub country: String,
        pub id: cynic::Id,
        pub name: String,
    }

    #[derive(cynic::QueryFragment, Debug, Clone, Serialize)]
    #[cynic(graphql_type = "TXTRecord")]
    pub struct TxtRecord {
        pub id: cynic::Id,
        pub created_at: DateTime,
        pub updated_at: DateTime,
        pub deleted_at: Option<DateTime>,
        pub name: Option<String>,
        pub text: String,
        pub ttl: Option<i32>,
        pub data: String,

        pub domain: DnsDomain,
    }

    #[derive(cynic::QueryFragment, Debug, Clone, Serialize)]
    #[cynic(graphql_type = "SSHFPRecord")]
    pub struct SshfpRecord {
        pub id: cynic::Id,
        pub created_at: DateTime,
        pub updated_at: DateTime,
        pub deleted_at: Option<DateTime>,
        pub name: Option<String>,
        pub text: String,
        pub ttl: Option<i32>,
        #[cynic(rename = "type")]
        pub type_: DnsmanagerSshFingerprintRecordTypeChoices,
        pub algorithm: DnsmanagerSshFingerprintRecordAlgorithmChoices,
        pub fingerprint: String,

        pub domain: DnsDomain,
    }

    #[derive(cynic::QueryFragment, Debug, Clone, Serialize)]
    #[cynic(graphql_type = "SRVRecord")]
    pub struct SrvRecord {
        pub id: cynic::Id,
        pub created_at: DateTime,
        pub updated_at: DateTime,
        pub deleted_at: Option<DateTime>,
        pub name: Option<String>,
        pub text: String,
        pub ttl: Option<i32>,
        pub service: String,
        pub protocol: String,
        pub priority: i32,
        pub weight: i32,
        pub port: i32,
        pub target: String,

        pub domain: DnsDomain,
    }

    #[derive(cynic::QueryFragment, Debug, Clone, Serialize)]
    #[cynic(graphql_type = "SOARecord")]
    pub struct SoaRecord {
        pub id: cynic::Id,
        pub created_at: DateTime,
        pub updated_at: DateTime,
        pub deleted_at: Option<DateTime>,
        pub name: Option<String>,
        pub text: String,
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

    #[derive(cynic::QueryVariables, Debug, Clone)]
    pub struct GetAllDomainsVariables {
        pub after: Option<String>,
        pub first: Option<i32>,
        pub namespace: Option<String>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetAllDomainsVariables")]
    pub struct GetAllDomains {
        #[arguments(
            first: $first,
            after: $after,
            namespace: $namespace,
        )]
        #[cynic(rename = "getAllDomains")]
        pub get_all_domains: DnsDomainConnection,
    }

    #[derive(cynic::QueryFragment, Debug, Clone, Serialize)]
    #[cynic(graphql_type = "PTRRecord")]
    pub struct PtrRecord {
        pub id: cynic::Id,
        pub created_at: DateTime,
        pub updated_at: DateTime,
        pub deleted_at: Option<DateTime>,
        pub name: Option<String>,
        pub text: String,
        pub ttl: Option<i32>,
        pub ptrdname: String,

        pub domain: DnsDomain,
    }

    #[derive(cynic::QueryFragment, Debug, Clone, Serialize)]
    #[cynic(graphql_type = "NSRecord")]
    pub struct NsRecord {
        pub id: cynic::Id,
        pub created_at: DateTime,
        pub updated_at: DateTime,
        pub deleted_at: Option<DateTime>,
        pub name: Option<String>,
        pub text: String,
        pub ttl: Option<i32>,
        pub nsdname: String,

        pub domain: DnsDomain,
    }

    #[derive(cynic::QueryFragment, Debug, Clone, Serialize)]
    #[cynic(graphql_type = "MXRecord")]
    pub struct MxRecord {
        pub id: cynic::Id,
        pub created_at: DateTime,
        pub updated_at: DateTime,
        pub deleted_at: Option<DateTime>,
        pub name: Option<String>,
        pub text: String,
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

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "DNSDomainConnection")]
    pub struct DnsDomainConnection {
        pub page_info: PageInfo,
        pub edges: Vec<Option<DnsDomainEdge>>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "DNSDomainEdge")]
    pub struct DnsDomainEdge {
        pub node: Option<DnsDomain>,
    }

    #[derive(cynic::QueryFragment, Debug, Clone, Serialize)]
    #[cynic(graphql_type = "DNAMERecord")]
    pub struct DNameRecord {
        pub id: cynic::Id,
        pub created_at: DateTime,
        pub updated_at: DateTime,
        pub deleted_at: Option<DateTime>,
        pub name: Option<String>,
        pub text: String,
        pub ttl: Option<i32>,
        pub d_name: String,

        pub domain: DnsDomain,
    }

    #[derive(cynic::QueryFragment, Debug, Clone, Serialize)]
    #[cynic(graphql_type = "CNAMERecord")]
    pub struct CNameRecord {
        pub id: cynic::Id,
        pub created_at: DateTime,
        pub updated_at: DateTime,
        pub deleted_at: Option<DateTime>,
        pub name: Option<String>,
        pub text: String,
        pub ttl: Option<i32>,
        pub c_name: String,

        pub domain: DnsDomain,
    }

    #[derive(cynic::QueryFragment, Debug, Clone, Serialize)]
    #[cynic(graphql_type = "CAARecord")]
    pub struct CaaRecord {
        pub id: cynic::Id,
        pub created_at: DateTime,
        pub updated_at: DateTime,
        pub deleted_at: Option<DateTime>,
        pub name: Option<String>,
        pub text: String,
        pub ttl: Option<i32>,
        pub value: String,
        pub flags: i32,
        pub tag: DnsmanagerCertificationAuthorityAuthorizationRecordTagChoices,

        pub domain: DnsDomain,
    }

    #[derive(cynic::QueryFragment, Debug, Clone, Serialize)]
    #[cynic(graphql_type = "ARecord")]
    pub struct ARecord {
        pub id: cynic::Id,
        pub created_at: DateTime,
        pub updated_at: DateTime,
        pub deleted_at: Option<DateTime>,
        pub name: Option<String>,
        pub text: String,
        pub ttl: Option<i32>,
        pub address: String,
        pub domain: DnsDomain,
    }

    #[derive(cynic::QueryFragment, Debug, Clone, Serialize)]
    #[cynic(graphql_type = "AAAARecord")]
    pub struct AaaaRecord {
        pub id: cynic::Id,
        pub created_at: DateTime,
        pub updated_at: DateTime,
        pub deleted_at: Option<DateTime>,
        pub name: Option<String>,
        pub text: String,
        pub ttl: Option<i32>,
        pub address: String,
        pub domain: DnsDomain,
    }

    #[derive(cynic::InlineFragments, Debug, Clone, Serialize)]
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
        pub fn name(&self) -> Option<&str> {
            match self {
                DnsRecord::A(record) => record.name.as_deref(),
                DnsRecord::AAAA(record) => record.name.as_deref(),
                DnsRecord::CName(record) => record.name.as_deref(),
                DnsRecord::Txt(record) => record.name.as_deref(),
                DnsRecord::Mx(record) => record.name.as_deref(),
                DnsRecord::Ns(record) => record.name.as_deref(),
                DnsRecord::CAA(record) => record.name.as_deref(),
                DnsRecord::DName(record) => record.name.as_deref(),
                DnsRecord::Ptr(record) => record.name.as_deref(),
                DnsRecord::Soa(record) => record.name.as_deref(),
                DnsRecord::Srv(record) => record.name.as_deref(),
                DnsRecord::Sshfp(record) => record.name.as_deref(),
                DnsRecord::Unknown => None,
            }
        }
        pub fn ttl(&self) -> Option<i32> {
            match self {
                DnsRecord::A(record) => record.ttl,
                DnsRecord::AAAA(record) => record.ttl,
                DnsRecord::CName(record) => record.ttl,
                DnsRecord::Txt(record) => record.ttl,
                DnsRecord::Mx(record) => record.ttl,
                DnsRecord::Ns(record) => record.ttl,
                DnsRecord::CAA(record) => record.ttl,
                DnsRecord::DName(record) => record.ttl,
                DnsRecord::Ptr(record) => record.ttl,
                DnsRecord::Soa(record) => record.ttl,
                DnsRecord::Srv(record) => record.ttl,
                DnsRecord::Sshfp(record) => record.ttl,
                DnsRecord::Unknown => None,
            }
        }

        pub fn text(&self) -> &str {
            match self {
                DnsRecord::A(record) => record.text.as_str(),
                DnsRecord::AAAA(record) => record.text.as_str(),
                DnsRecord::CName(record) => record.text.as_str(),
                DnsRecord::Txt(record) => record.text.as_str(),
                DnsRecord::Mx(record) => record.text.as_str(),
                DnsRecord::Ns(record) => record.text.as_str(),
                DnsRecord::CAA(record) => record.text.as_str(),
                DnsRecord::DName(record) => record.text.as_str(),
                DnsRecord::Ptr(record) => record.text.as_str(),
                DnsRecord::Soa(record) => record.text.as_str(),
                DnsRecord::Srv(record) => record.text.as_str(),
                DnsRecord::Sshfp(record) => record.text.as_str(),
                DnsRecord::Unknown => "",
            }
        }
        pub fn record_type(&self) -> &str {
            match self {
                DnsRecord::A(_) => "A",
                DnsRecord::AAAA(_) => "AAAA",
                DnsRecord::CName(_) => "CNAME",
                DnsRecord::Txt(_) => "TXT",
                DnsRecord::Mx(_) => "MX",
                DnsRecord::Ns(_) => "NS",
                DnsRecord::CAA(_) => "CAA",
                DnsRecord::DName(_) => "DNAME",
                DnsRecord::Ptr(_) => "PTR",
                DnsRecord::Soa(_) => "SOA",
                DnsRecord::Srv(_) => "SRV",
                DnsRecord::Sshfp(_) => "SSHFP",
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

        pub fn created_at(&self) -> Option<&DateTime> {
            match self {
                DnsRecord::A(record) => Some(&record.created_at),
                DnsRecord::AAAA(record) => Some(&record.created_at),
                DnsRecord::CName(record) => Some(&record.created_at),
                DnsRecord::Txt(record) => Some(&record.created_at),
                DnsRecord::Mx(record) => Some(&record.created_at),
                DnsRecord::Ns(record) => Some(&record.created_at),
                DnsRecord::CAA(record) => Some(&record.created_at),
                DnsRecord::DName(record) => Some(&record.created_at),
                DnsRecord::Ptr(record) => Some(&record.created_at),
                DnsRecord::Soa(record) => Some(&record.created_at),
                DnsRecord::Srv(record) => Some(&record.created_at),
                DnsRecord::Sshfp(record) => Some(&record.created_at),
                DnsRecord::Unknown => None,
            }
        }

        pub fn updated_at(&self) -> Option<&DateTime> {
            match self {
                Self::A(record) => Some(&record.updated_at),
                Self::AAAA(record) => Some(&record.updated_at),
                Self::CName(record) => Some(&record.updated_at),
                Self::Txt(record) => Some(&record.updated_at),
                Self::Mx(record) => Some(&record.updated_at),
                Self::Ns(record) => Some(&record.updated_at),
                Self::CAA(record) => Some(&record.updated_at),
                Self::DName(record) => Some(&record.updated_at),
                Self::Ptr(record) => Some(&record.updated_at),
                Self::Soa(record) => Some(&record.updated_at),
                Self::Srv(record) => Some(&record.updated_at),
                Self::Sshfp(record) => Some(&record.updated_at),
                Self::Unknown => None,
            }
        }

        pub fn deleted_at(&self) -> Option<&DateTime> {
            match self {
                Self::A(record) => record.deleted_at.as_ref(),
                Self::AAAA(record) => record.deleted_at.as_ref(),
                Self::CName(record) => record.deleted_at.as_ref(),
                Self::Txt(record) => record.deleted_at.as_ref(),
                Self::Mx(record) => record.deleted_at.as_ref(),
                Self::Ns(record) => record.deleted_at.as_ref(),
                Self::CAA(record) => record.deleted_at.as_ref(),
                Self::DName(record) => record.deleted_at.as_ref(),
                Self::Ptr(record) => record.deleted_at.as_ref(),
                Self::Soa(record) => record.deleted_at.as_ref(),
                Self::Srv(record) => record.deleted_at.as_ref(),
                Self::Sshfp(record) => record.deleted_at.as_ref(),
                Self::Unknown => None,
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
    pub struct GetDomainWithZoneFile {
        #[arguments(name: $domain)]
        pub get_domain: Option<DnsDomainWithZoneFile>,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Query", variables = "GetDomainVars")]
    pub struct GetDomainWithRecords {
        #[arguments(name: $domain)]
        pub get_domain: Option<DnsDomainWithRecords>,
    }

    #[derive(cynic::QueryFragment, Debug, Clone, Serialize)]
    #[cynic(graphql_type = "DNSDomain")]
    pub struct DnsDomain {
        pub id: cynic::Id,
        pub name: String,
        pub slug: String,
        pub owner: Owner,
    }

    #[derive(cynic::QueryFragment, Debug, Clone, Serialize)]
    #[cynic(graphql_type = "DNSDomain")]
    pub struct DnsDomainWithZoneFile {
        pub id: cynic::Id,
        pub name: String,
        pub slug: String,
        pub zone_file: String,
    }

    #[derive(cynic::QueryFragment, Debug, Clone, Serialize)]
    #[cynic(graphql_type = "DNSDomain")]
    pub struct DnsDomainWithRecords {
        pub id: cynic::Id,
        pub name: String,
        pub slug: String,
        pub records: Option<Vec<Option<DnsRecord>>>,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct PurgeCacheForAppVersionVars {
        pub id: cynic::Id,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct PurgeCacheForAppVersionPayload {
        pub app_version: DeployAppVersion,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(graphql_type = "Mutation", variables = "PurgeCacheForAppVersionVars")]
    pub struct PurgeCacheForAppVersion {
        #[arguments(input: {id: $id})]
        pub purge_cache_for_app_version: Option<PurgeCacheForAppVersionPayload>,
    }

    #[derive(cynic::Scalar, Debug, Clone)]
    #[cynic(graphql_type = "URL")]
    pub struct Url(pub String);

    #[derive(cynic::Scalar, Debug, Clone)]
    pub struct BigInt(pub i64);

    #[derive(cynic::Enum, Clone, Copy, Debug, PartialEq, Eq)]
    pub enum ProgrammingLanguage {
        Python,
        Javascript,
    }

    /// A library that exposes bindings to a Wasmer package.
    #[derive(Debug, Clone)]
    pub struct Bindings {
        /// A unique ID specifying this set of bindings.
        pub id: String,
        /// The URL which can be used to download the files that were generated
        /// (typically as a `*.tar.gz` file).
        pub url: String,
        /// The programming language these bindings are written in.
        pub language: ProgrammingLanguage,
        /// The generator used to generate these bindings.
        pub generator: BindingsGenerator,
    }

    #[derive(cynic::QueryVariables, Debug, Clone)]
    pub struct GetBindingsQueryVariables<'a> {
        pub name: &'a str,
        pub version: Option<&'a str>,
    }

    #[derive(cynic::QueryFragment, Debug, Clone)]
    #[cynic(graphql_type = "Query", variables = "GetBindingsQueryVariables")]
    pub struct GetBindingsQuery {
        #[arguments(name: $name, version: $version)]
        #[cynic(rename = "getPackageVersion")]
        pub package_version: Option<PackageBindingsVersion>,
    }

    #[derive(cynic::QueryFragment, Debug, Clone)]
    #[cynic(graphql_type = "PackageVersion")]
    pub struct PackageBindingsVersion {
        pub bindings: Vec<Option<PackageVersionLanguageBinding>>,
    }

    #[derive(cynic::QueryFragment, Debug, Clone)]
    pub struct BindingsGenerator {
        pub package_version: PackageVersion,
        pub command_name: String,
    }

    #[derive(cynic::QueryFragment, Debug, Clone)]
    pub struct PackageVersionLanguageBinding {
        pub id: cynic::Id,
        pub language: ProgrammingLanguage,
        pub url: String,
        pub generator: BindingsGenerator,
        pub __typename: String,
    }

    #[derive(cynic::QueryVariables, Debug)]
    pub struct PackageVersionReadySubscriptionVariables {
        pub package_version_id: cynic::Id,
    }

    #[derive(cynic::QueryFragment, Debug)]
    #[cynic(
        graphql_type = "Subscription",
        variables = "PackageVersionReadySubscriptionVariables"
    )]
    pub struct PackageVersionReadySubscription {
        #[arguments(packageVersionId: $package_version_id)]
        pub package_version_ready: PackageVersionReadyResponse,
    }

    #[derive(cynic::QueryFragment, Debug)]
    pub struct PackageVersionReadyResponse {
        pub state: PackageVersionState,
        pub success: bool,
    }

    #[derive(cynic::Enum, Clone, Copy, Debug)]
    pub enum PackageVersionState {
        WebcGenerated,
        BindingsGenerated,
        NativeExesGenerated,
    }

    #[derive(cynic::InlineFragments, Debug, Clone)]
    #[cynic(graphql_type = "Node", variables = "GetDeployAppVersionsByIdVars")]
    pub enum NodeDeployAppVersions {
        DeployApp(Box<DeployAppVersionsById>),
        #[cynic(fallback)]
        Unknown,
    }

    impl NodeDeployAppVersions {
        pub fn into_app(self) -> Option<DeployAppVersionsById> {
            match self {
                Self::DeployApp(v) => Some(*v),
                _ => None,
            }
        }
    }

    #[derive(cynic::InlineFragments, Debug)]
    pub enum Node {
        DeployApp(Box<DeployApp>),
        DeployAppVersion(Box<DeployAppVersion>),
        AutobuildRepository(Box<AutobuildRepository>),
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
