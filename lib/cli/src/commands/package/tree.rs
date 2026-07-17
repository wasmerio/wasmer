use std::sync::Arc;

use anyhow::{Context, Result};
use semver::{Op, Version, VersionReq};
use wasmer_config::package::{PackageId, PackageIdent, PackageSource, Tag};
use wasmer_wasix::runtime::resolver::{
    BackendSource, Dependency, DependencyGraph, FileSystemSource, MultiSource, Source, WebSource,
};

use crate::{
    commands::AsyncCliCommand,
    config::WasmerEnv,
    utils::{WAPM_SOURCE_CACHE_TIMEOUT, registry_query_cache_dir},
};

/// Print a package's resolved dependency tree.
#[derive(clap::Parser, Debug)]
pub struct PackageTree {
    #[clap(flatten)]
    pub env: WasmerEnv,

    /// Disable the cache for package metadata queries.
    #[clap(long = "disable-cache")]
    disable_cache: bool,

    /// The package identifier to resolve, either a package name or sha256 hash.
    package: PackageIdent,
}

#[async_trait::async_trait]
impl AsyncCliCommand for PackageTree {
    type Output = ();

    async fn run_async(self) -> Result<Self::Output> {
        let source = self.prepare_source()?;
        let package = PackageSource::Ident(self.package);

        let root_summary = source.latest(&package).await.map_err(|error| {
            wasmer_wasix::runtime::resolver::ResolveError::Registry {
                package: package.clone(),
                error,
            }
        })?;
        let root_id = root_summary.package_id();

        let graph = wasmer_wasix::runtime::resolver::resolve_dependency_graph(
            &root_id,
            &root_summary.pkg,
            &source,
        )
        .await
        .context("Dependency graph resolution failed")?;

        println!("{}", format_root(&package, graph.id()));
        print_dependencies(&graph, graph.id(), "");

        wasmer_wasix::runtime::resolver::validate_dependency_graph(&graph)
            .context("Dependency graph cannot be unified")?;

        Ok(())
    }
}

impl PackageTree {
    fn prepare_source(&self) -> Result<MultiSource> {
        let client =
            wasmer_wasix::http::default_http_client().context("No HTTP client available")?;
        let client = Arc::new(client);

        let mut source = MultiSource::default();

        let registry_endpoint = self.env.registry_endpoint()?;
        let mut registry = BackendSource::new(registry_endpoint.clone(), client.clone());
        if !self.disable_cache {
            let cache_dir = registry_query_cache_dir(self.env.cache_dir(), &registry_endpoint);
            registry = registry.with_local_cache(cache_dir, WAPM_SOURCE_CACHE_TIMEOUT);
        }
        if let Some(token) = self.env.token() {
            registry = registry.with_auth_token(token);
        }
        source.add_source(registry);

        let downloads_cache_dir = self.env.cache_dir().join("downloads");
        source.add_source(WebSource::new(downloads_cache_dir, client));
        source.add_source(FileSystemSource::default());

        Ok(source)
    }
}

fn print_dependencies(graph: &DependencyGraph, package_id: &PackageId, prefix: &str) {
    let dependencies = dependency_edges(graph, package_id);

    for (index, (_alias, dependency, resolved_id)) in dependencies.iter().enumerate() {
        let is_last = index + 1 == dependencies.len();
        let connector = if is_last { "`-- " } else { "|-- " };

        println!(
            "{prefix}{connector}{}",
            format_dependency(dependency, resolved_id)
        );

        let child_prefix = if is_last { "    " } else { "|   " };
        print_dependencies(graph, resolved_id, &format!("{prefix}{child_prefix}"));
    }
}

fn dependency_edges(
    graph: &DependencyGraph,
    package_id: &PackageId,
) -> Vec<(String, Dependency, PackageId)> {
    let dependency_ids = graph
        .iter_dependencies()
        .find(|(id, _)| *id == package_id)
        .map(|(_, dependencies)| dependencies)
        .unwrap_or_default();

    let package = &graph[package_id].pkg;

    dependency_ids
        .into_iter()
        .filter_map(|(alias, resolved_id)| {
            let dependency = package
                .dependencies
                .iter()
                .find(|dependency| dependency.alias() == alias)?;

            Some((alias.to_string(), dependency.clone(), resolved_id.clone()))
        })
        .collect()
}

fn format_root(specified: &PackageSource, resolved_id: &PackageId) -> String {
    if is_fixed_to_resolved(specified, resolved_id) {
        resolved_id.to_string()
    } else {
        format!("{specified} -> {resolved_id}")
    }
}

fn format_dependency(dependency: &Dependency, resolved_id: &PackageId) -> String {
    if let (PackageSource::Ident(PackageIdent::Named(specified)), PackageId::Named(resolved)) =
        (&dependency.pkg, resolved_id)
        && specified.full_name() == resolved.full_name
    {
        let specified_version = specified
            .tag
            .as_ref()
            .map_or("*".to_string(), |tag| tag.to_string());

        if is_fixed_to_resolved(&dependency.pkg, resolved_id) {
            return format!("{}@{specified_version}", specified.full_name());
        }

        return format!(
            "{}@{specified_version}=>{}",
            specified.full_name(),
            resolved.version
        );
    }

    if is_fixed_to_resolved(&dependency.pkg, resolved_id) {
        dependency.pkg.to_string()
    } else {
        format!("{} -> {resolved_id}", dependency.pkg)
    }
}

fn is_fixed_to_resolved(specified: &PackageSource, resolved_id: &PackageId) -> bool {
    match (specified, resolved_id) {
        (PackageSource::Ident(PackageIdent::Hash(specified)), PackageId::Hash(resolved)) => {
            specified == resolved
        }
        (PackageSource::Ident(PackageIdent::Named(specified)), PackageId::Named(resolved)) => {
            if specified.full_name() != resolved.full_name {
                return false;
            }

            match &specified.tag {
                Some(Tag::Named(tag)) => tag == &resolved.version.to_string(),
                Some(Tag::VersionReq(req)) => version_req_is_exact(req, &resolved.version),
                None => false,
            }
        }
        _ => false,
    }
}

fn version_req_is_exact(req: &VersionReq, version: &Version) -> bool {
    let [comparator] = req.comparators.as_slice() else {
        return false;
    };

    comparator.op == Op::Exact
        && comparator.major == version.major
        && comparator.minor == Some(version.minor)
        && comparator.patch == Some(version.patch)
        && comparator.pre == version.pre
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_dependency_never_includes_aliases() {
        let dependency = Dependency {
            alias: "logger".to_string(),
            pkg: PackageSource::from(
                wasmer_config::package::NamedPackageIdent::try_from_full_name_and_version(
                    "wasmer/log",
                    "^1",
                )
                .unwrap(),
            ),
        };
        let resolved_id = PackageId::new_named("wasmer/log", "1.2.3".parse().unwrap());

        assert_eq!(
            format_dependency(&dependency, &resolved_id),
            "wasmer/log@^1=>1.2.3"
        );
    }
}
