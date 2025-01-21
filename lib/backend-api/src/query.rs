use std::{collections::HashSet, time::Duration};

use anyhow::{bail, Context};
use cynic::{MutationBuilder, QueryBuilder};
use futures::StreamExt;
use merge_streams::MergeStreams;
use time::OffsetDateTime;
use tracing::Instrument;
use url::Url;
use wasmer_config::package::PackageIdent;
use wasmer_package::utils::from_bytes;
use webc::Container;

use crate::{
    types::{self, *},
    GraphQLApiFailure, WasmerClient,
};

/// Rotate the s3 secrets tied to an app given its id.
pub async fn rotate_s3_secrets(
    client: &WasmerClient,
    app_id: types::Id,
) -> Result<(), anyhow::Error> {
    client
        .run_graphql_strict(types::RotateS3SecretsForApp::build(
            RotateS3SecretsForAppVariables { id: app_id },
        ))
        .await?;

    Ok(())
}

pub async fn viewer_can_deploy_to_namespace(
    client: &WasmerClient,
    owner_name: &str,
) -> Result<bool, anyhow::Error> {
    client
        .run_graphql_strict(types::ViewerCan::build(ViewerCanVariables {
            action: OwnerAction::DeployApp,
            owner_name,
        }))
        .await
        .map(|v| v.viewer_can)
}

pub async fn redeploy_app_by_id(
    client: &WasmerClient,
    app_id: impl Into<String>,
) -> Result<Option<DeployApp>, anyhow::Error> {
    client
        .run_graphql_strict(types::RedeployActiveApp::build(
            RedeployActiveAppVariables {
                id: types::Id::from(app_id),
            },
        ))
        .await
        .map(|v| v.redeploy_active_version.map(|v| v.app))
}

/// List all bindings associated with a particular package.
///
/// If a version number isn't provided, this will default to the most recently
/// published version.
pub async fn list_bindings(
    client: &WasmerClient,
    name: &str,
    version: Option<&str>,
) -> Result<Vec<Bindings>, anyhow::Error> {
    client
        .run_graphql_strict(types::GetBindingsQuery::build(GetBindingsQueryVariables {
            name,
            version,
        }))
        .await
        .and_then(|b| {
            b.package_version
                .ok_or(anyhow::anyhow!("No bindings found!"))
        })
        .map(|v| {
            let mut bindings_packages = Vec::new();

            for b in v.bindings.into_iter().flatten() {
                let pkg = Bindings {
                    id: b.id.into_inner(),
                    url: b.url,
                    language: b.language,
                    generator: b.generator,
                };
                bindings_packages.push(pkg);
            }

            bindings_packages
        })
}

/// Revoke an existing token
pub async fn revoke_token(
    client: &WasmerClient,
    token: String,
) -> Result<Option<bool>, anyhow::Error> {
    client
        .run_graphql_strict(types::RevokeToken::build(RevokeTokenVariables { token }))
        .await
        .map(|v| v.revoke_api_token.and_then(|v| v.success))
}

/// Generate a new Nonce
///
/// Takes a name and a callbackUrl and returns a nonce
pub async fn create_nonce(
    client: &WasmerClient,
    name: String,
    callback_url: String,
) -> Result<Option<Nonce>, anyhow::Error> {
    client
        .run_graphql_strict(types::CreateNewNonce::build(CreateNewNonceVariables {
            callback_url,
            name,
        }))
        .await
        .map(|v| v.new_nonce.map(|v| v.nonce))
}

pub async fn get_app_secret_value_by_id(
    client: &WasmerClient,
    secret_id: impl Into<String>,
) -> Result<Option<String>, anyhow::Error> {
    client
        .run_graphql_strict(types::GetAppSecretValue::build(
            GetAppSecretValueVariables {
                id: types::Id::from(secret_id),
            },
        ))
        .await
        .map(|v| v.get_secret_value)
}

pub async fn get_app_secret_by_name(
    client: &WasmerClient,
    app_id: impl Into<String>,
    name: impl Into<String>,
) -> Result<Option<Secret>, anyhow::Error> {
    client
        .run_graphql_strict(types::GetAppSecret::build(GetAppSecretVariables {
            app_id: types::Id::from(app_id),
            secret_name: name.into(),
        }))
        .await
        .map(|v| v.get_app_secret)
}

/// Update or create an app secret.
pub async fn upsert_app_secret(
    client: &WasmerClient,
    app_id: impl Into<String>,
    name: impl Into<String>,
    value: impl Into<String>,
) -> Result<Option<UpsertAppSecretPayload>, anyhow::Error> {
    client
        .run_graphql_strict(types::UpsertAppSecret::build(UpsertAppSecretVariables {
            app_id: cynic::Id::from(app_id.into()),
            name: name.into().as_str(),
            value: value.into().as_str(),
        }))
        .await
        .map(|v| v.upsert_app_secret)
}

/// Update or create app secrets in bulk.
pub async fn upsert_app_secrets(
    client: &WasmerClient,
    app_id: impl Into<String>,
    secrets: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
) -> Result<Option<UpsertAppSecretsPayload>, anyhow::Error> {
    client
        .run_graphql_strict(types::UpsertAppSecrets::build(UpsertAppSecretsVariables {
            app_id: cynic::Id::from(app_id.into()),
            secrets: Some(
                secrets
                    .into_iter()
                    .map(|(name, value)| SecretInput {
                        name: name.into(),
                        value: value.into(),
                    })
                    .collect(),
            ),
        }))
        .await
        .map(|v| v.upsert_app_secrets)
}

/// Load all secrets of an app.
///
/// Will paginate through all versions and return them in a single list.
pub async fn get_all_app_secrets_filtered(
    client: &WasmerClient,
    app_id: impl Into<String>,
    names: impl IntoIterator<Item = impl Into<String>>,
) -> Result<Vec<Secret>, anyhow::Error> {
    let mut vars = GetAllAppSecretsVariables {
        after: None,
        app_id: types::Id::from(app_id),
        before: None,
        first: None,
        last: None,
        offset: None,
        names: Some(names.into_iter().map(|s| s.into()).collect()),
    };

    let mut all_secrets = Vec::<Secret>::new();

    loop {
        let page = get_app_secrets(client, vars.clone()).await?;
        if page.edges.is_empty() {
            break;
        }

        for edge in page.edges {
            let edge = match edge {
                Some(edge) => edge,
                None => continue,
            };
            let version = match edge.node {
                Some(item) => item,
                None => continue,
            };

            all_secrets.push(version);

            // Update pagination.
            vars.after = Some(edge.cursor);
        }
    }

    Ok(all_secrets)
}

/// Retrieve volumes for an app.
pub async fn get_app_volumes(
    client: &WasmerClient,
    owner: impl Into<String>,
    name: impl Into<String>,
) -> Result<Vec<types::AppVersionVolume>, anyhow::Error> {
    let vars = types::GetAppVolumesVars {
        owner: owner.into(),
        name: name.into(),
    };
    let res = client
        .run_graphql_strict(types::GetAppVolumes::build(vars))
        .await?;
    let volumes = res
        .get_deploy_app
        .context("app not found")?
        .active_version
        .and_then(|v| v.volumes)
        .unwrap_or_default()
        .into_iter()
        .flatten()
        .collect();
    Ok(volumes)
}

/// Load the S3 credentials.
///
/// S3 can be used to get access to an apps volumes.
pub async fn get_app_s3_credentials(
    client: &WasmerClient,
    app_id: impl Into<String>,
) -> Result<types::S3Credentials, anyhow::Error> {
    let app_id = app_id.into();

    // Firt load the app to get the s3 url.
    let app1 = get_app_by_id(client, app_id.clone()).await?;

    let vars = types::GetDeployAppVars {
        owner: app1.owner.global_name,
        name: app1.name,
    };
    client
        .run_graphql_strict(types::GetDeployAppS3Credentials::build(vars))
        .await?
        .get_deploy_app
        .context("app not found")?
        .s3_credentials
        .context("app does not have S3 credentials")
}

/// Load all available regions.
///
/// Will paginate through all versions and return them in a single list.
pub async fn get_all_app_regions(client: &WasmerClient) -> Result<Vec<AppRegion>, anyhow::Error> {
    let mut vars = GetAllAppRegionsVariables {
        after: None,
        before: None,
        first: None,
        last: None,
        offset: None,
    };

    let mut all_regions = Vec::<AppRegion>::new();

    loop {
        let page = get_regions(client, vars.clone()).await?;
        if page.edges.is_empty() {
            break;
        }

        for edge in page.edges {
            let edge = match edge {
                Some(edge) => edge,
                None => continue,
            };
            let version = match edge.node {
                Some(item) => item,
                None => continue,
            };

            all_regions.push(version);

            // Update pagination.
            vars.after = Some(edge.cursor);
        }
    }

    Ok(all_regions)
}

/// Retrieve regions.
pub async fn get_regions(
    client: &WasmerClient,
    vars: GetAllAppRegionsVariables,
) -> Result<AppRegionConnection, anyhow::Error> {
    let res = client
        .run_graphql_strict(types::GetAllAppRegions::build(vars))
        .await?;
    Ok(res.get_app_regions)
}

/// Load all secrets of an app.
///
/// Will paginate through all versions and return them in a single list.
pub async fn get_all_app_secrets(
    client: &WasmerClient,
    app_id: impl Into<String>,
) -> Result<Vec<Secret>, anyhow::Error> {
    let mut vars = GetAllAppSecretsVariables {
        after: None,
        app_id: types::Id::from(app_id),
        before: None,
        first: None,
        last: None,
        offset: None,
        names: None,
    };

    let mut all_secrets = Vec::<Secret>::new();

    loop {
        let page = get_app_secrets(client, vars.clone()).await?;
        if page.edges.is_empty() {
            break;
        }

        for edge in page.edges {
            let edge = match edge {
                Some(edge) => edge,
                None => continue,
            };
            let version = match edge.node {
                Some(item) => item,
                None => continue,
            };

            all_secrets.push(version);

            // Update pagination.
            vars.after = Some(edge.cursor);
        }
    }

    Ok(all_secrets)
}

/// Retrieve secrets for an app.
pub async fn get_app_secrets(
    client: &WasmerClient,
    vars: GetAllAppSecretsVariables,
) -> Result<SecretConnection, anyhow::Error> {
    let res = client
        .run_graphql_strict(types::GetAllAppSecrets::build(vars))
        .await?;
    res.get_app_secrets.context("app not found")
}

pub async fn delete_app_secret(
    client: &WasmerClient,
    secret_id: impl Into<String>,
) -> Result<Option<DeleteAppSecretPayload>, anyhow::Error> {
    client
        .run_graphql_strict(types::DeleteAppSecret::build(DeleteAppSecretVariables {
            id: types::Id::from(secret_id.into()),
        }))
        .await
        .map(|v| v.delete_app_secret)
}

/// Load a webc package from the registry.
///
/// NOTE: this uses the public URL instead of the download URL available through
/// the API, and should not be used where possible.
pub async fn fetch_webc_package(
    client: &WasmerClient,
    ident: &PackageIdent,
    default_registry: &Url,
) -> Result<Container, anyhow::Error> {
    let url = match ident {
        PackageIdent::Named(n) => Url::parse(&format!(
            "{default_registry}/{}:{}",
            n.full_name(),
            n.version_or_default()
        ))?,
        PackageIdent::Hash(h) => match get_package_release(client, &h.to_string()).await? {
            Some(webc) => Url::parse(&webc.webc_url)?,
            None => anyhow::bail!("Could not find package with hash '{}'", h),
        },
    };

    let data = client
        .client
        .get(url)
        .header(reqwest::header::USER_AGENT, &client.user_agent)
        .header(reqwest::header::ACCEPT, "application/webc")
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;

    from_bytes(data).context("failed to parse webc package")
}

/// Fetch app templates.
pub async fn fetch_app_template_from_slug(
    client: &WasmerClient,
    slug: String,
) -> Result<Option<types::AppTemplate>, anyhow::Error> {
    client
        .run_graphql_strict(types::GetAppTemplateFromSlug::build(
            GetAppTemplateFromSlugVariables { slug },
        ))
        .await
        .map(|v| v.get_app_template)
}

/// Fetch app templates.
pub async fn fetch_app_templates_from_framework(
    client: &WasmerClient,
    framework_slug: String,
    first: i32,
    after: Option<String>,
    sort_by: Option<types::AppTemplatesSortBy>,
) -> Result<Option<types::AppTemplateConnection>, anyhow::Error> {
    client
        .run_graphql_strict(types::GetAppTemplatesFromFramework::build(
            GetAppTemplatesFromFrameworkVars {
                framework_slug,
                first,
                after,
                sort_by,
            },
        ))
        .await
        .map(|r| r.get_app_templates)
}

/// Fetch app templates.
pub async fn fetch_app_templates(
    client: &WasmerClient,
    category_slug: String,
    first: i32,
    after: Option<String>,
    sort_by: Option<types::AppTemplatesSortBy>,
) -> Result<Option<types::AppTemplateConnection>, anyhow::Error> {
    client
        .run_graphql_strict(types::GetAppTemplates::build(GetAppTemplatesVars {
            category_slug,
            first,
            after,
            sort_by,
        }))
        .await
        .map(|r| r.get_app_templates)
}

/// Fetch all app templates by paginating through the responses.
///
/// Will fetch at most `max` templates.
pub fn fetch_all_app_templates(
    client: &WasmerClient,
    page_size: i32,
    sort_by: Option<types::AppTemplatesSortBy>,
) -> impl futures::Stream<Item = Result<Vec<types::AppTemplate>, anyhow::Error>> + '_ {
    let vars = GetAppTemplatesVars {
        category_slug: String::new(),
        first: page_size,
        sort_by,
        after: None,
    };

    futures::stream::try_unfold(
        Some(vars),
        move |vars: Option<types::GetAppTemplatesVars>| async move {
            let vars = match vars {
                Some(vars) => vars,
                None => return Ok(None),
            };

            let con = client
                .run_graphql_strict(types::GetAppTemplates::build(vars.clone()))
                .await?
                .get_app_templates
                .context("backend did not return any data")?;

            let items = con
                .edges
                .into_iter()
                .flatten()
                .filter_map(|edge| edge.node)
                .collect::<Vec<_>>();

            let next_cursor = con
                .page_info
                .end_cursor
                .filter(|_| con.page_info.has_next_page);

            let next_vars = next_cursor.map(|after| types::GetAppTemplatesVars {
                after: Some(after),
                ..vars
            });

            #[allow(clippy::type_complexity)]
            let res: Result<
                Option<(Vec<types::AppTemplate>, Option<types::GetAppTemplatesVars>)>,
                anyhow::Error,
            > = Ok(Some((items, next_vars)));

            res
        },
    )
}

/// Fetch all app templates by paginating through the responses.
///
/// Will fetch at most `max` templates.
pub fn fetch_all_app_templates_from_language(
    client: &WasmerClient,
    page_size: i32,
    sort_by: Option<types::AppTemplatesSortBy>,
    language: String,
) -> impl futures::Stream<Item = Result<Vec<types::AppTemplate>, anyhow::Error>> + '_ {
    let vars = GetAppTemplatesFromLanguageVars {
        language_slug: language.clone().to_string(),
        first: page_size,
        sort_by,
        after: None,
    };

    futures::stream::try_unfold(
        Some(vars),
        move |vars: Option<types::GetAppTemplatesFromLanguageVars>| async move {
            let vars = match vars {
                Some(vars) => vars,
                None => return Ok(None),
            };

            let con = client
                .run_graphql_strict(types::GetAppTemplatesFromLanguage::build(vars.clone()))
                .await?
                .get_app_templates
                .context("backend did not return any data")?;

            let items = con
                .edges
                .into_iter()
                .flatten()
                .filter_map(|edge| edge.node)
                .collect::<Vec<_>>();

            let next_cursor = con
                .page_info
                .end_cursor
                .filter(|_| con.page_info.has_next_page);

            let next_vars = next_cursor.map(|after| types::GetAppTemplatesFromLanguageVars {
                after: Some(after),
                ..vars
            });

            #[allow(clippy::type_complexity)]
            let res: Result<
                Option<(
                    Vec<types::AppTemplate>,
                    Option<types::GetAppTemplatesFromLanguageVars>,
                )>,
                anyhow::Error,
            > = Ok(Some((items, next_vars)));

            res
        },
    )
}

/// Fetch languages from available app templates.
pub async fn fetch_app_template_languages(
    client: &WasmerClient,
    after: Option<String>,
    first: Option<i32>,
) -> Result<Option<types::TemplateLanguageConnection>, anyhow::Error> {
    client
        .run_graphql_strict(types::GetTemplateLanguages::build(
            GetTemplateLanguagesVars { after, first },
        ))
        .await
        .map(|r| r.get_template_languages)
}

/// Fetch all languages from available app templates by paginating through the responses.
///
/// Will fetch at most `max` templates.
pub fn fetch_all_app_template_languages(
    client: &WasmerClient,
    page_size: Option<i32>,
) -> impl futures::Stream<Item = Result<Vec<types::TemplateLanguage>, anyhow::Error>> + '_ {
    let vars = GetTemplateLanguagesVars {
        after: None,
        first: page_size,
    };

    futures::stream::try_unfold(
        Some(vars),
        move |vars: Option<types::GetTemplateLanguagesVars>| async move {
            let vars = match vars {
                Some(vars) => vars,
                None => return Ok(None),
            };

            let con = client
                .run_graphql_strict(types::GetTemplateLanguages::build(vars.clone()))
                .await?
                .get_template_languages
                .context("backend did not return any data")?;

            let items = con
                .edges
                .into_iter()
                .flatten()
                .filter_map(|edge| edge.node)
                .collect::<Vec<_>>();

            let next_cursor = con
                .page_info
                .end_cursor
                .filter(|_| con.page_info.has_next_page);

            let next_vars = next_cursor.map(|after| types::GetTemplateLanguagesVars {
                after: Some(after),
                ..vars
            });

            #[allow(clippy::type_complexity)]
            let res: Result<
                Option<(
                    Vec<types::TemplateLanguage>,
                    Option<types::GetTemplateLanguagesVars>,
                )>,
                anyhow::Error,
            > = Ok(Some((items, next_vars)));

            res
        },
    )
}

/// Fetch all app templates by paginating through the responses.
///
/// Will fetch at most `max` templates.
pub fn fetch_all_app_templates_from_framework(
    client: &WasmerClient,
    page_size: i32,
    sort_by: Option<types::AppTemplatesSortBy>,
    framework: String,
) -> impl futures::Stream<Item = Result<Vec<types::AppTemplate>, anyhow::Error>> + '_ {
    let vars = GetAppTemplatesFromFrameworkVars {
        framework_slug: framework.clone().to_string(),
        first: page_size,
        sort_by,
        after: None,
    };

    futures::stream::try_unfold(
        Some(vars),
        move |vars: Option<types::GetAppTemplatesFromFrameworkVars>| async move {
            let vars = match vars {
                Some(vars) => vars,
                None => return Ok(None),
            };

            let con = client
                .run_graphql_strict(types::GetAppTemplatesFromFramework::build(vars.clone()))
                .await?
                .get_app_templates
                .context("backend did not return any data")?;

            let items = con
                .edges
                .into_iter()
                .flatten()
                .filter_map(|edge| edge.node)
                .collect::<Vec<_>>();

            let next_cursor = con
                .page_info
                .end_cursor
                .filter(|_| con.page_info.has_next_page);

            let next_vars = next_cursor.map(|after| types::GetAppTemplatesFromFrameworkVars {
                after: Some(after),
                ..vars
            });

            #[allow(clippy::type_complexity)]
            let res: Result<
                Option<(
                    Vec<types::AppTemplate>,
                    Option<types::GetAppTemplatesFromFrameworkVars>,
                )>,
                anyhow::Error,
            > = Ok(Some((items, next_vars)));

            res
        },
    )
}

/// Fetch frameworks from available app templates.
pub async fn fetch_app_template_frameworks(
    client: &WasmerClient,
    after: Option<String>,
    first: Option<i32>,
) -> Result<Option<types::TemplateFrameworkConnection>, anyhow::Error> {
    client
        .run_graphql_strict(types::GetTemplateFrameworks::build(
            GetTemplateFrameworksVars { after, first },
        ))
        .await
        .map(|r| r.get_template_frameworks)
}

/// Fetch all frameworks from available app templates by paginating through the responses.
///
/// Will fetch at most `max` templates.
pub fn fetch_all_app_template_frameworks(
    client: &WasmerClient,
    page_size: Option<i32>,
) -> impl futures::Stream<Item = Result<Vec<types::TemplateFramework>, anyhow::Error>> + '_ {
    let vars = GetTemplateFrameworksVars {
        after: None,
        first: page_size,
    };

    futures::stream::try_unfold(
        Some(vars),
        move |vars: Option<types::GetTemplateFrameworksVars>| async move {
            let vars = match vars {
                Some(vars) => vars,
                None => return Ok(None),
            };

            let con = client
                .run_graphql_strict(types::GetTemplateFrameworks::build(vars.clone()))
                .await?
                .get_template_frameworks
                .context("backend did not return any data")?;

            let items = con
                .edges
                .into_iter()
                .flatten()
                .filter_map(|edge| edge.node)
                .collect::<Vec<_>>();

            let next_cursor = con
                .page_info
                .end_cursor
                .filter(|_| con.page_info.has_next_page);

            let next_vars = next_cursor.map(|after| types::GetTemplateFrameworksVars {
                after: Some(after),
                ..vars
            });

            #[allow(clippy::type_complexity)]
            let res: Result<
                Option<(
                    Vec<types::TemplateFramework>,
                    Option<types::GetTemplateFrameworksVars>,
                )>,
                anyhow::Error,
            > = Ok(Some((items, next_vars)));

            res
        },
    )
}

/// Get a signed URL to upload packages.
pub async fn get_signed_url_for_package_upload(
    client: &WasmerClient,
    expires_after_seconds: Option<i32>,
    filename: Option<&str>,
    name: Option<&str>,
    version: Option<&str>,
) -> Result<Option<SignedUrl>, anyhow::Error> {
    client
        .run_graphql_strict(types::GetSignedUrlForPackageUpload::build(
            GetSignedUrlForPackageUploadVariables {
                expires_after_seconds,
                filename,
                name,
                version,
            },
        ))
        .await
        .map(|r| r.get_signed_url_for_package_upload)
}
/// Push a package to the registry.
pub async fn push_package_release(
    client: &WasmerClient,
    name: Option<&str>,
    namespace: &str,
    signed_url: &str,
    private: Option<bool>,
) -> Result<Option<PushPackageReleasePayload>, anyhow::Error> {
    client
        .run_graphql_strict(types::PushPackageRelease::build(
            types::PushPackageReleaseVariables {
                name,
                namespace,
                private,
                signed_url,
            },
        ))
        .await
        .map(|r| r.push_package_release)
}

#[allow(clippy::too_many_arguments)]
pub async fn tag_package_release(
    client: &WasmerClient,
    description: Option<&str>,
    homepage: Option<&str>,
    license: Option<&str>,
    license_file: Option<&str>,
    manifest: Option<&str>,
    name: &str,
    namespace: Option<&str>,
    package_release_id: &cynic::Id,
    private: Option<bool>,
    readme: Option<&str>,
    repository: Option<&str>,
    version: &str,
) -> Result<Option<TagPackageReleasePayload>, anyhow::Error> {
    client
        .run_graphql_strict(types::TagPackageRelease::build(
            types::TagPackageReleaseVariables {
                description,
                homepage,
                license,
                license_file,
                manifest,
                name,
                namespace,
                package_release_id,
                private,
                readme,
                repository,
                version,
            },
        ))
        .await
        .map(|r| r.tag_package_release)
}

/// Get the currently logged in user.
pub async fn current_user(client: &WasmerClient) -> Result<Option<types::User>, anyhow::Error> {
    client
        .run_graphql(types::GetCurrentUser::build(()))
        .await
        .map(|x| x.viewer)
}

/// Get the currently logged in user, together with all accessible namespaces.
///
/// You can optionally filter the namespaces by the user role.
pub async fn current_user_with_namespaces(
    client: &WasmerClient,
    namespace_role: Option<types::GrapheneRole>,
) -> Result<types::UserWithNamespaces, anyhow::Error> {
    client
        .run_graphql(types::GetCurrentUserWithNamespaces::build(
            types::GetCurrentUserWithNamespacesVars { namespace_role },
        ))
        .await?
        .viewer
        .context("not logged in")
}

/// Retrieve an app.
pub async fn get_app(
    client: &WasmerClient,
    owner: String,
    name: String,
) -> Result<Option<types::DeployApp>, anyhow::Error> {
    client
        .run_graphql(types::GetDeployApp::build(types::GetDeployAppVars {
            name,
            owner,
        }))
        .await
        .map(|x| x.get_deploy_app)
}

/// Retrieve an app by its global alias.
pub async fn get_app_by_alias(
    client: &WasmerClient,
    alias: String,
) -> Result<Option<types::DeployApp>, anyhow::Error> {
    client
        .run_graphql(types::GetDeployAppByAlias::build(
            types::GetDeployAppByAliasVars { alias },
        ))
        .await
        .map(|x| x.get_app_by_global_alias)
}

/// Retrieve an app version.
pub async fn get_app_version(
    client: &WasmerClient,
    owner: String,
    name: String,
    version: String,
) -> Result<Option<types::DeployAppVersion>, anyhow::Error> {
    client
        .run_graphql(types::GetDeployAppVersion::build(
            types::GetDeployAppVersionVars {
                name,
                owner,
                version,
            },
        ))
        .await
        .map(|x| x.get_deploy_app_version)
}

/// Retrieve an app together with a specific version.
pub async fn get_app_with_version(
    client: &WasmerClient,
    owner: String,
    name: String,
    version: String,
) -> Result<GetDeployAppAndVersion, anyhow::Error> {
    client
        .run_graphql(types::GetDeployAppAndVersion::build(
            types::GetDeployAppAndVersionVars {
                name,
                owner,
                version,
            },
        ))
        .await
}

/// Retrieve an app together with a specific version.
pub async fn get_app_and_package_by_name(
    client: &WasmerClient,
    vars: types::GetPackageAndAppVars,
) -> Result<(Option<types::Package>, Option<types::DeployApp>), anyhow::Error> {
    let res = client
        .run_graphql(types::GetPackageAndApp::build(vars))
        .await?;
    Ok((res.get_package, res.get_deploy_app))
}

/// Retrieve apps.
pub async fn get_deploy_apps(
    client: &WasmerClient,
    vars: types::GetDeployAppsVars,
) -> Result<DeployAppConnection, anyhow::Error> {
    let res = client
        .run_graphql(types::GetDeployApps::build(vars))
        .await?;
    res.get_deploy_apps.context("no apps returned")
}

/// Retrieve apps as a stream that will automatically paginate.
pub fn get_deploy_apps_stream(
    client: &WasmerClient,
    vars: types::GetDeployAppsVars,
) -> impl futures::Stream<Item = Result<Vec<DeployApp>, anyhow::Error>> + '_ {
    futures::stream::try_unfold(
        Some(vars),
        move |vars: Option<types::GetDeployAppsVars>| async move {
            let vars = match vars {
                Some(vars) => vars,
                None => return Ok(None),
            };

            let page = get_deploy_apps(client, vars.clone()).await?;

            let end_cursor = page.page_info.end_cursor;

            let items = page
                .edges
                .into_iter()
                .filter_map(|x| x.and_then(|x| x.node))
                .collect::<Vec<_>>();

            let new_vars = end_cursor.map(|c| types::GetDeployAppsVars {
                after: Some(c),
                ..vars
            });

            Ok(Some((items, new_vars)))
        },
    )
}

/// Retrieve versions for an app.
pub async fn get_deploy_app_versions(
    client: &WasmerClient,
    vars: GetDeployAppVersionsVars,
) -> Result<DeployAppVersionConnection, anyhow::Error> {
    let res = client
        .run_graphql_strict(types::GetDeployAppVersions::build(vars))
        .await?;
    let versions = res.get_deploy_app.context("app not found")?.versions;
    Ok(versions)
}

/// Get app deployments for an app.
pub async fn app_deployments(
    client: &WasmerClient,
    vars: types::GetAppDeploymentsVariables,
) -> Result<Vec<types::Deployment>, anyhow::Error> {
    let res = client
        .run_graphql_strict(types::GetAppDeployments::build(vars))
        .await?;
    let builds = res
        .get_deploy_app
        .and_then(|x| x.deployments)
        .context("no data returned")?
        .edges
        .into_iter()
        .flatten()
        .filter_map(|x| x.node)
        .collect();

    Ok(builds)
}

/// Get an app deployment by ID.
pub async fn app_deployment(
    client: &WasmerClient,
    id: String,
) -> Result<types::AutobuildRepository, anyhow::Error> {
    let node = get_node(client, id.clone())
        .await?
        .with_context(|| format!("app deployment with id '{id}' not found"))?;
    match node {
        types::Node::AutobuildRepository(x) => Ok(*x),
        _ => anyhow::bail!("invalid node type returned"),
    }
}

/// Load all versions of an app.
///
/// Will paginate through all versions and return them in a single list.
pub async fn all_app_versions(
    client: &WasmerClient,
    owner: String,
    name: String,
) -> Result<Vec<DeployAppVersion>, anyhow::Error> {
    let mut vars = GetDeployAppVersionsVars {
        owner,
        name,
        offset: None,
        before: None,
        after: None,
        first: Some(10),
        last: None,
        sort_by: None,
    };

    let mut all_versions = Vec::<DeployAppVersion>::new();

    loop {
        let page = get_deploy_app_versions(client, vars.clone()).await?;
        if page.edges.is_empty() {
            break;
        }

        for edge in page.edges {
            let edge = match edge {
                Some(edge) => edge,
                None => continue,
            };
            let version = match edge.node {
                Some(item) => item,
                None => continue,
            };

            // Sanity check to avoid duplication.
            if all_versions.iter().any(|v| v.id == version.id) == false {
                all_versions.push(version);
            }

            // Update pagination.
            vars.after = Some(edge.cursor);
        }
    }

    Ok(all_versions)
}

/// Retrieve versions for an app.
pub async fn get_deploy_app_versions_by_id(
    client: &WasmerClient,
    vars: types::GetDeployAppVersionsByIdVars,
) -> Result<DeployAppVersionConnection, anyhow::Error> {
    let res = client
        .run_graphql_strict(types::GetDeployAppVersionsById::build(vars))
        .await?;
    let versions = res
        .node
        .context("app not found")?
        .into_app()
        .context("invalid node type returned")?
        .versions;
    Ok(versions)
}

/// Load all versions of an app id.
///
/// Will paginate through all versions and return them in a single list.
pub async fn all_app_versions_by_id(
    client: &WasmerClient,
    app_id: impl Into<String>,
) -> Result<Vec<DeployAppVersion>, anyhow::Error> {
    let mut vars = types::GetDeployAppVersionsByIdVars {
        id: cynic::Id::new(app_id),
        offset: None,
        before: None,
        after: None,
        first: Some(10),
        last: None,
        sort_by: None,
    };

    let mut all_versions = Vec::<DeployAppVersion>::new();

    loop {
        let page = get_deploy_app_versions_by_id(client, vars.clone()).await?;
        if page.edges.is_empty() {
            break;
        }

        for edge in page.edges {
            let edge = match edge {
                Some(edge) => edge,
                None => continue,
            };
            let version = match edge.node {
                Some(item) => item,
                None => continue,
            };

            // Sanity check to avoid duplication.
            if all_versions.iter().any(|v| v.id == version.id) == false {
                all_versions.push(version);
            }

            // Update pagination.
            vars.after = Some(edge.cursor);
        }
    }

    Ok(all_versions)
}

/// Activate a particular version of an app.
pub async fn app_version_activate(
    client: &WasmerClient,
    version: String,
) -> Result<DeployApp, anyhow::Error> {
    let res = client
        .run_graphql_strict(types::MarkAppVersionAsActive::build(
            types::MarkAppVersionAsActiveVars {
                input: types::MarkAppVersionAsActiveInput {
                    app_version: version.into(),
                },
            },
        ))
        .await?;
    res.mark_app_version_as_active
        .context("app not found")
        .map(|x| x.app)
}

/// Retrieve a node based on its global id.
pub async fn get_node(
    client: &WasmerClient,
    id: String,
) -> Result<Option<types::Node>, anyhow::Error> {
    client
        .run_graphql(types::GetNode::build(types::GetNodeVars { id: id.into() }))
        .await
        .map(|x| x.node)
}

/// Retrieve an app by its global id.
pub async fn get_app_by_id(
    client: &WasmerClient,
    app_id: String,
) -> Result<DeployApp, anyhow::Error> {
    get_app_by_id_opt(client, app_id)
        .await?
        .context("app not found")
}

/// Retrieve an app by its global id.
pub async fn get_app_by_id_opt(
    client: &WasmerClient,
    app_id: String,
) -> Result<Option<DeployApp>, anyhow::Error> {
    let app_opt = client
        .run_graphql(types::GetDeployAppById::build(
            types::GetDeployAppByIdVars {
                app_id: app_id.into(),
            },
        ))
        .await?
        .app;

    if let Some(app) = app_opt {
        let app = app.into_deploy_app().context("app conversion failed")?;
        Ok(Some(app))
    } else {
        Ok(None)
    }
}

/// Retrieve an app together with a specific version.
pub async fn get_app_with_version_by_id(
    client: &WasmerClient,
    app_id: String,
    version_id: String,
) -> Result<(DeployApp, DeployAppVersion), anyhow::Error> {
    let res = client
        .run_graphql(types::GetDeployAppAndVersionById::build(
            types::GetDeployAppAndVersionByIdVars {
                app_id: app_id.into(),
                version_id: version_id.into(),
            },
        ))
        .await?;

    let app = res
        .app
        .context("app not found")?
        .into_deploy_app()
        .context("app conversion failed")?;
    let version = res
        .version
        .context("version not found")?
        .into_deploy_app_version()
        .context("version conversion failed")?;

    Ok((app, version))
}

/// Retrieve an app version by its global id.
pub async fn get_app_version_by_id(
    client: &WasmerClient,
    version_id: String,
) -> Result<DeployAppVersion, anyhow::Error> {
    client
        .run_graphql(types::GetDeployAppVersionById::build(
            types::GetDeployAppVersionByIdVars {
                version_id: version_id.into(),
            },
        ))
        .await?
        .version
        .context("app not found")?
        .into_deploy_app_version()
        .context("app version conversion failed")
}

pub async fn get_app_version_by_id_with_app(
    client: &WasmerClient,
    version_id: String,
) -> Result<(DeployApp, DeployAppVersion), anyhow::Error> {
    let version = client
        .run_graphql(types::GetDeployAppVersionById::build(
            types::GetDeployAppVersionByIdVars {
                version_id: version_id.into(),
            },
        ))
        .await?
        .version
        .context("app not found")?
        .into_deploy_app_version()
        .context("app version conversion failed")?;

    let app_id = version
        .app
        .as_ref()
        .context("could not load app for version")?
        .id
        .clone();

    let app = get_app_by_id(client, app_id.into_inner()).await?;

    Ok((app, version))
}

/// List all apps that are accessible by the current user.
///
/// NOTE: this will only include the first pages and does not provide pagination.
pub async fn user_apps(
    client: &WasmerClient,
    sort: types::DeployAppsSortBy,
) -> impl futures::Stream<Item = Result<Vec<types::DeployApp>, anyhow::Error>> + '_ {
    futures::stream::try_unfold(None, move |cursor| async move {
        let user = client
            .run_graphql(types::GetCurrentUserWithApps::build(
                GetCurrentUserWithAppsVars {
                    after: cursor,
                    sort: Some(sort),
                },
            ))
            .await?
            .viewer
            .context("not logged in")?;

        let apps: Vec<_> = user
            .apps
            .edges
            .into_iter()
            .flatten()
            .filter_map(|x| x.node)
            .collect();

        let cursor = user.apps.page_info.end_cursor;

        if apps.is_empty() {
            Ok(None)
        } else {
            Ok(Some((apps, cursor)))
        }
    })
}

/// List all apps that are accessible by the current user.
pub async fn user_accessible_apps(
    client: &WasmerClient,
    sort: types::DeployAppsSortBy,
) -> Result<
    impl futures::Stream<Item = Result<Vec<types::DeployApp>, anyhow::Error>> + '_,
    anyhow::Error,
> {
    let user_apps = user_apps(client, sort).await;

    // Get all aps in user-accessible namespaces.
    let namespace_res = client
        .run_graphql(types::GetCurrentUserWithNamespaces::build(
            types::GetCurrentUserWithNamespacesVars {
                namespace_role: None,
            },
        ))
        .await?;
    let active_user = namespace_res.viewer.context("not logged in")?;
    let namespace_names = active_user
        .namespaces
        .edges
        .iter()
        .filter_map(|edge| edge.as_ref())
        .filter_map(|edge| edge.node.as_ref())
        .map(|node| node.name.clone())
        .collect::<Vec<_>>();

    let mut ns_apps = vec![];
    for ns in namespace_names {
        let apps = namespace_apps(client, ns, sort).await;
        ns_apps.push(apps);
    }

    Ok((user_apps, ns_apps.merge()).merge())
}

/// Get apps for a specific namespace.
///
/// NOTE: only retrieves the first page and does not do pagination.
pub async fn namespace_apps(
    client: &WasmerClient,
    namespace: String,
    sort: types::DeployAppsSortBy,
) -> impl futures::Stream<Item = Result<Vec<types::DeployApp>, anyhow::Error>> + '_ {
    let namespace = namespace.clone();

    futures::stream::try_unfold((None, namespace), move |(cursor, namespace)| async move {
        let res = client
            .run_graphql(types::GetNamespaceApps::build(GetNamespaceAppsVars {
                name: namespace.to_string(),
                after: cursor,
                sort: Some(sort),
            }))
            .await?;

        let ns = res
            .get_namespace
            .with_context(|| format!("failed to get namespace '{namespace}'"))?;

        let apps: Vec<_> = ns
            .apps
            .edges
            .into_iter()
            .flatten()
            .filter_map(|x| x.node)
            .collect();

        let cursor = ns.apps.page_info.end_cursor;

        if apps.is_empty() {
            Ok(None)
        } else {
            Ok(Some((apps, (cursor, namespace))))
        }
    })
}

/// Publish a new app (version).
pub async fn publish_deploy_app(
    client: &WasmerClient,
    vars: PublishDeployAppVars,
) -> Result<DeployAppVersion, anyhow::Error> {
    let res = client
        .run_graphql_raw(types::PublishDeployApp::build(vars))
        .await?;

    if let Some(app) = res
        .data
        .and_then(|d| d.publish_deploy_app)
        .map(|d| d.deploy_app_version)
    {
        Ok(app)
    } else {
        Err(GraphQLApiFailure::from_errors(
            "could not publish app",
            res.errors,
        ))
    }
}

/// Delete an app.
pub async fn delete_app(client: &WasmerClient, app_id: String) -> Result<(), anyhow::Error> {
    let res = client
        .run_graphql_strict(types::DeleteApp::build(types::DeleteAppVars {
            app_id: app_id.into(),
        }))
        .await?
        .delete_app
        .context("API did not return data for the delete_app mutation")?;

    if !res.success {
        bail!("App deletion failed for an unknown reason");
    }

    Ok(())
}

/// Get all namespaces accessible by the current user.
pub async fn user_namespaces(
    client: &WasmerClient,
) -> Result<Vec<types::Namespace>, anyhow::Error> {
    let user = client
        .run_graphql(types::GetCurrentUserWithNamespaces::build(
            types::GetCurrentUserWithNamespacesVars {
                namespace_role: None,
            },
        ))
        .await?
        .viewer
        .context("not logged in")?;

    let ns = user
        .namespaces
        .edges
        .into_iter()
        .flatten()
        // .filter_map(|x| x)
        .filter_map(|x| x.node)
        .collect();

    Ok(ns)
}

/// Retrieve a namespace by its name.
pub async fn get_namespace(
    client: &WasmerClient,
    name: String,
) -> Result<Option<types::Namespace>, anyhow::Error> {
    client
        .run_graphql(types::GetNamespace::build(types::GetNamespaceVars { name }))
        .await
        .map(|x| x.get_namespace)
}

/// Create a new namespace.
pub async fn create_namespace(
    client: &WasmerClient,
    vars: CreateNamespaceVars,
) -> Result<types::Namespace, anyhow::Error> {
    client
        .run_graphql(types::CreateNamespace::build(vars))
        .await?
        .create_namespace
        .map(|x| x.namespace)
        .context("no namespace returned")
}

/// Retrieve a package by its name.
pub async fn get_package(
    client: &WasmerClient,
    name: String,
) -> Result<Option<types::Package>, anyhow::Error> {
    client
        .run_graphql_strict(types::GetPackage::build(types::GetPackageVars { name }))
        .await
        .map(|x| x.get_package)
}

/// Retrieve a package version by its name.
pub async fn get_package_version(
    client: &WasmerClient,
    name: String,
    version: String,
) -> Result<Option<types::PackageVersionWithPackage>, anyhow::Error> {
    client
        .run_graphql_strict(types::GetPackageVersion::build(
            types::GetPackageVersionVars { name, version },
        ))
        .await
        .map(|x| x.get_package_version)
}

/// Retrieve package versions for an app.
pub async fn get_package_versions(
    client: &WasmerClient,
    vars: types::AllPackageVersionsVars,
) -> Result<PackageVersionConnection, anyhow::Error> {
    let res = client
        .run_graphql(types::GetAllPackageVersions::build(vars))
        .await?;
    Ok(res.all_package_versions)
}

/// Retrieve a package release by hash.
pub async fn get_package_release(
    client: &WasmerClient,
    hash: &str,
) -> Result<Option<types::PackageWebc>, anyhow::Error> {
    let hash = hash.trim_start_matches("sha256:");
    client
        .run_graphql_strict(types::GetPackageRelease::build(
            types::GetPackageReleaseVars {
                hash: hash.to_string(),
            },
        ))
        .await
        .map(|x| x.get_package_release)
}

pub async fn get_package_releases(
    client: &WasmerClient,
    vars: types::AllPackageReleasesVars,
) -> Result<types::PackageWebcConnection, anyhow::Error> {
    let res = client
        .run_graphql(types::GetAllPackageReleases::build(vars))
        .await?;
    Ok(res.all_package_releases)
}

/// Retrieve all versions of a package as a stream that auto-paginates.
pub fn get_package_versions_stream(
    client: &WasmerClient,
    vars: types::AllPackageVersionsVars,
) -> impl futures::Stream<Item = Result<Vec<types::PackageVersionWithPackage>, anyhow::Error>> + '_
{
    futures::stream::try_unfold(
        Some(vars),
        move |vars: Option<types::AllPackageVersionsVars>| async move {
            let vars = match vars {
                Some(vars) => vars,
                None => return Ok(None),
            };

            let page = get_package_versions(client, vars.clone()).await?;

            let end_cursor = page.page_info.end_cursor;

            let items = page
                .edges
                .into_iter()
                .filter_map(|x| x.and_then(|x| x.node))
                .collect::<Vec<_>>();

            let new_vars = end_cursor.map(|cursor| types::AllPackageVersionsVars {
                after: Some(cursor),
                ..vars
            });

            Ok(Some((items, new_vars)))
        },
    )
}

/// Retrieve all package releases as a stream.
pub fn get_package_releases_stream(
    client: &WasmerClient,
    vars: types::AllPackageReleasesVars,
) -> impl futures::Stream<Item = Result<Vec<types::PackageWebc>, anyhow::Error>> + '_ {
    futures::stream::try_unfold(
        Some(vars),
        move |vars: Option<types::AllPackageReleasesVars>| async move {
            let vars = match vars {
                Some(vars) => vars,
                None => return Ok(None),
            };

            let page = get_package_releases(client, vars.clone()).await?;

            let end_cursor = page.page_info.end_cursor;

            let items = page
                .edges
                .into_iter()
                .filter_map(|x| x.and_then(|x| x.node))
                .collect::<Vec<_>>();

            let new_vars = end_cursor.map(|cursor| types::AllPackageReleasesVars {
                after: Some(cursor),
                ..vars
            });

            Ok(Some((items, new_vars)))
        },
    )
}

#[derive(Debug, PartialEq)]
pub enum TokenKind {
    SSH,
}

pub async fn generate_deploy_config_token_raw(
    client: &WasmerClient,
    token_kind: TokenKind,
) -> Result<String, anyhow::Error> {
    let res = client
        .run_graphql(types::GenerateDeployConfigToken::build(
            types::GenerateDeployConfigTokenVars {
                input: match token_kind {
                    TokenKind::SSH => "{}".to_string(),
                },
            },
        ))
        .await?;

    res.generate_deploy_config_token
        .map(|x| x.token)
        .context("no token returned")
}

/// Get pages of logs associated with an application that lie within the
/// specified date range.
// NOTE: this is not public due to severe usability issues.
// The stream can loop forever due to re-fetching the same logs over and over.
#[tracing::instrument(skip_all, level = "debug")]
#[allow(clippy::let_with_type_underscore)]
#[allow(clippy::too_many_arguments)]
fn get_app_logs(
    client: &WasmerClient,
    name: String,
    owner: String,
    tag: Option<String>,
    start: OffsetDateTime,
    end: Option<OffsetDateTime>,
    watch: bool,
    streams: Option<Vec<LogStream>>,
    request_id: Option<String>,
    instance_ids: Option<Vec<String>>,
) -> impl futures::Stream<Item = Result<Vec<Log>, anyhow::Error>> + '_ {
    // Note: the backend will limit responses to a certain number of log
    // messages, so we use try_unfold() to keep calling it until we stop getting
    // new log messages.
    let span = tracing::Span::current();

    futures::stream::try_unfold(start, move |start| {
        let variables = types::GetDeployAppLogsVars {
            name: name.clone(),
            owner: owner.clone(),
            version: tag.clone(),
            first: Some(100),
            starting_from: unix_timestamp(start),
            until: end.map(unix_timestamp),
            streams: streams.clone(),
            request_id: request_id.clone(),
            instance_ids: instance_ids.clone(),
        };

        let fut = async move {
            loop {
                let deploy_app_version = client
                    .run_graphql(types::GetDeployAppLogs::build(variables.clone()))
                    .await?
                    .get_deploy_app_version
                    .context("app version not found")?;

                let page: Vec<_> = deploy_app_version
                    .logs
                    .edges
                    .into_iter()
                    .flatten()
                    .filter_map(|edge| edge.node)
                    .collect();

                if page.is_empty() {
                    if watch {
                        /*
                         * [TODO]: The resolution here should be configurable.
                         */

                        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
                        std::thread::sleep(Duration::from_secs(1));

                        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
                        tokio::time::sleep(Duration::from_secs(1)).await;

                        continue;
                    }

                    break Ok(None);
                } else {
                    let last_message = page.last().expect("The page is non-empty");
                    let timestamp = last_message.timestamp;
                    // NOTE: adding 1 microsecond to the timestamp to avoid fetching
                    // the last message again.
                    let timestamp = OffsetDateTime::from_unix_timestamp_nanos(timestamp as i128)
                        .with_context(|| {
                            format!("Unable to interpret {timestamp} as a unix timestamp")
                        })?;

                    // FIXME: We need a better way to tell the backend "give me the
                    // next set of logs". Adding 1 nanosecond could theoretically
                    // mean we miss messages if multiple log messages arrived at
                    // the same nanosecond and the page ended midway.

                    let next_timestamp = timestamp + Duration::from_nanos(1_000);

                    break Ok(Some((page, next_timestamp)));
                }
            }
        };

        fut.instrument(span.clone())
    })
}

/// Get pages of logs associated with an application that lie within the
/// specified date range.
///
/// In contrast to [`get_app_logs`], this function collects the stream into a
/// final vector.
#[tracing::instrument(skip_all, level = "debug")]
#[allow(clippy::let_with_type_underscore)]
#[allow(clippy::too_many_arguments)]
pub async fn get_app_logs_paginated(
    client: &WasmerClient,
    name: String,
    owner: String,
    tag: Option<String>,
    start: OffsetDateTime,
    end: Option<OffsetDateTime>,
    watch: bool,
    streams: Option<Vec<LogStream>>,
) -> impl futures::Stream<Item = Result<Vec<Log>, anyhow::Error>> + '_ {
    let stream = get_app_logs(
        client, name, owner, tag, start, end, watch, streams, None, None,
    );

    stream.map(|res| {
        let mut logs = Vec::new();
        let mut hasher = HashSet::new();
        let mut page = res?;

        // Prevent duplicates.
        // TODO: don't clone the message, just hash it.
        page.retain(|log| hasher.insert((log.message.clone(), log.timestamp.round() as i128)));

        logs.extend(page);

        Ok(logs)
    })
}

/// Get pages of logs associated with an application that lie within the
/// specified date range with a specific instance identifier.
///
/// In contrast to [`get_app_logs`], this function collects the stream into a
/// final vector.
#[tracing::instrument(skip_all, level = "debug")]
#[allow(clippy::let_with_type_underscore)]
#[allow(clippy::too_many_arguments)]
pub async fn get_app_logs_paginated_filter_instance(
    client: &WasmerClient,
    name: String,
    owner: String,
    tag: Option<String>,
    start: OffsetDateTime,
    end: Option<OffsetDateTime>,
    watch: bool,
    streams: Option<Vec<LogStream>>,
    instance_ids: Vec<String>,
) -> impl futures::Stream<Item = Result<Vec<Log>, anyhow::Error>> + '_ {
    let stream = get_app_logs(
        client,
        name,
        owner,
        tag,
        start,
        end,
        watch,
        streams,
        None,
        Some(instance_ids),
    );

    stream.map(|res| {
        let mut logs = Vec::new();
        let mut hasher = HashSet::new();
        let mut page = res?;

        // Prevent duplicates.
        // TODO: don't clone the message, just hash it.
        page.retain(|log| hasher.insert((log.message.clone(), log.timestamp.round() as i128)));

        logs.extend(page);

        Ok(logs)
    })
}

/// Get pages of logs associated with an specific request for application that lie within the
/// specified date range.
///
/// In contrast to [`get_app_logs`], this function collects the stream into a
/// final vector.
#[tracing::instrument(skip_all, level = "debug")]
#[allow(clippy::let_with_type_underscore)]
#[allow(clippy::too_many_arguments)]
pub async fn get_app_logs_paginated_filter_request(
    client: &WasmerClient,
    name: String,
    owner: String,
    tag: Option<String>,
    start: OffsetDateTime,
    end: Option<OffsetDateTime>,
    watch: bool,
    streams: Option<Vec<LogStream>>,
    request_id: String,
) -> impl futures::Stream<Item = Result<Vec<Log>, anyhow::Error>> + '_ {
    let stream = get_app_logs(
        client,
        name,
        owner,
        tag,
        start,
        end,
        watch,
        streams,
        Some(request_id),
        None,
    );

    stream.map(|res| {
        let mut logs = Vec::new();
        let mut hasher = HashSet::new();
        let mut page = res?;

        // Prevent duplicates.
        // TODO: don't clone the message, just hash it.
        page.retain(|log| hasher.insert((log.message.clone(), log.timestamp.round() as i128)));

        logs.extend(page);

        Ok(logs)
    })
}

/// Retrieve a domain by its name.
///
/// Specify with_records to also retrieve all records for the domain.
pub async fn get_domain(
    client: &WasmerClient,
    domain: String,
) -> Result<Option<types::DnsDomain>, anyhow::Error> {
    let vars = types::GetDomainVars { domain };

    let opt = client
        .run_graphql(types::GetDomain::build(vars))
        .await
        .map_err(anyhow::Error::from)?
        .get_domain;
    Ok(opt)
}

/// Retrieve a domain by its name.
///
/// Specify with_records to also retrieve all records for the domain.
pub async fn get_domain_zone_file(
    client: &WasmerClient,
    domain: String,
) -> Result<Option<types::DnsDomainWithZoneFile>, anyhow::Error> {
    let vars = types::GetDomainVars { domain };

    let opt = client
        .run_graphql(types::GetDomainWithZoneFile::build(vars))
        .await
        .map_err(anyhow::Error::from)?
        .get_domain;
    Ok(opt)
}

/// Retrieve a domain by its name, along with all it's records.
pub async fn get_domain_with_records(
    client: &WasmerClient,
    domain: String,
) -> Result<Option<types::DnsDomainWithRecords>, anyhow::Error> {
    let vars = types::GetDomainVars { domain };

    let opt = client
        .run_graphql(types::GetDomainWithRecords::build(vars))
        .await
        .map_err(anyhow::Error::from)?
        .get_domain;
    Ok(opt)
}

/// Register a new domain
pub async fn register_domain(
    client: &WasmerClient,
    name: String,
    namespace: Option<String>,
    import_records: Option<bool>,
) -> Result<types::DnsDomain, anyhow::Error> {
    let vars = types::RegisterDomainVars {
        name,
        namespace,
        import_records,
    };
    let opt = client
        .run_graphql_strict(types::RegisterDomain::build(vars))
        .await
        .map_err(anyhow::Error::from)?
        .register_domain
        .context("Domain registration failed")?
        .domain
        .context("Domain registration failed, no associatede domain found.")?;
    Ok(opt)
}

/// Retrieve all DNS records.
///
/// NOTE: this is a privileged operation that requires extra permissions.
pub async fn get_all_dns_records(
    client: &WasmerClient,
    vars: types::GetAllDnsRecordsVariables,
) -> Result<types::DnsRecordConnection, anyhow::Error> {
    client
        .run_graphql_strict(types::GetAllDnsRecords::build(vars))
        .await
        .map_err(anyhow::Error::from)
        .map(|x| x.get_all_dnsrecords)
}

/// Retrieve all DNS domains.
pub async fn get_all_domains(
    client: &WasmerClient,
    vars: types::GetAllDomainsVariables,
) -> Result<Vec<DnsDomain>, anyhow::Error> {
    let connection = client
        .run_graphql_strict(types::GetAllDomains::build(vars))
        .await
        .map_err(anyhow::Error::from)
        .map(|x| x.get_all_domains)
        .context("no domains returned")?;
    Ok(connection
        .edges
        .into_iter()
        .flatten()
        .filter_map(|x| x.node)
        .collect())
}

/// Retrieve a domain by its name.
///
/// Specify with_records to also retrieve all records for the domain.
pub fn get_all_dns_records_stream(
    client: &WasmerClient,
    vars: types::GetAllDnsRecordsVariables,
) -> impl futures::Stream<Item = Result<Vec<types::DnsRecord>, anyhow::Error>> + '_ {
    futures::stream::try_unfold(
        Some(vars),
        move |vars: Option<types::GetAllDnsRecordsVariables>| async move {
            let vars = match vars {
                Some(vars) => vars,
                None => return Ok(None),
            };

            let page = get_all_dns_records(client, vars.clone()).await?;

            let end_cursor = page.page_info.end_cursor;

            let items = page
                .edges
                .into_iter()
                .filter_map(|x| x.and_then(|x| x.node))
                .collect::<Vec<_>>();

            let new_vars = end_cursor.map(|c| types::GetAllDnsRecordsVariables {
                after: Some(c),
                ..vars
            });

            Ok(Some((items, new_vars)))
        },
    )
}

pub async fn purge_cache_for_app_version(
    client: &WasmerClient,
    vars: types::PurgeCacheForAppVersionVars,
) -> Result<(), anyhow::Error> {
    client
        .run_graphql_strict(types::PurgeCacheForAppVersion::build(vars))
        .await
        .map_err(anyhow::Error::from)
        .map(|x| x.purge_cache_for_app_version)
        .context("backend did not return data")?;

    Ok(())
}

/// Convert a [`OffsetDateTime`] to a unix timestamp that the WAPM backend
/// understands.
fn unix_timestamp(ts: OffsetDateTime) -> f64 {
    let nanos_per_second = 1_000_000_000;
    let timestamp = ts.unix_timestamp_nanos();
    let nanos = timestamp % nanos_per_second;
    let secs = timestamp / nanos_per_second;

    (secs as f64) + (nanos as f64 / nanos_per_second as f64)
}

/// Publish a new app (version).
pub async fn upsert_domain_from_zone_file(
    client: &WasmerClient,
    zone_file_contents: String,
    delete_missing_records: bool,
) -> Result<DnsDomain, anyhow::Error> {
    let vars = UpsertDomainFromZoneFileVars {
        zone_file: zone_file_contents,
        delete_missing_records: Some(delete_missing_records),
    };
    let res = client
        .run_graphql_strict(types::UpsertDomainFromZoneFile::build(vars))
        .await?;

    let domain = res
        .upsert_domain_from_zone_file
        .context("Upserting domain from zonefile failed")?
        .domain;

    Ok(domain)
}
