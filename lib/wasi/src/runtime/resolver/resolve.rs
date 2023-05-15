use std::collections::{HashMap, HashSet, VecDeque};

use anyhow::Error;
use semver::Version;

use crate::{
    bin_factory::BinaryPackage,
    runtime::{
        package_loader::PackageLoader,
        resolver::{DependencyGraph, Registry, Resolution, ResolvedPackage, Summary},
    },
};

pub async fn load_package_tree(
    _loader: &impl PackageLoader,
    _resolution: &Resolution,
) -> Result<BinaryPackage, Error> {
    todo!();
}

/// Given a [`RootPackage`], resolve its dependency graph and figure out
/// how it could be reconstituted.
pub async fn resolve(root: &Summary, registry: &impl Registry) -> Result<Resolution, Error> {
    let summaries = fetch_all_possible_dependencies(root, registry).await?;
    let graph = resolve_dependency_graph(root, summaries)?;
    let package = resolve_package(&graph)?;

    Ok(Resolution { graph, package })
}

fn resolve_dependency_graph(
    root: &Summary,
    summaries: HashMap<String, HashMap<Version, Summary>>,
) -> Result<DependencyGraph, Error> {
    Ok(DependencyGraph {
        root: root.package_id(),
        dependencies: HashMap::new(),
        summaries: summaries
            .into_values()
            .flat_map(|versions| versions.into_values())
            .map(|summary| (summary.package_id(), summary))
            .collect(),
    })
}

fn resolve_package(dependency_graph: &DependencyGraph) -> Result<ResolvedPackage, Error> {
    todo!();
}

/// Naively create a graph of all packages that could possibly be reached by the
/// root package.
async fn fetch_all_possible_dependencies(
    root: &Summary,
    registry: &impl Registry,
) -> Result<HashMap<String, HashMap<Version, Summary>>, Error> {
    let mut summaries_by_name: HashMap<String, HashMap<Version, Summary>> = HashMap::new();

    let mut to_fetch = VecDeque::new();
    let mut visited = HashSet::new();

    for dep in &root.dependencies {
        to_fetch.push_back(dep.pkg.clone());
    }

    while let Some(specifier) = to_fetch.pop_front() {
        if visited.contains(&specifier) {
            continue;
        }

        let matches = registry.query(&specifier).await?;
        visited.insert(specifier);

        to_fetch.extend(
            matches
                .iter()
                .flat_map(|s| s.dependencies.iter().map(|dep| dep.pkg.clone())),
        );

        for summary in matches {
            summaries_by_name
                .entry(summary.package_name.clone())
                .or_default()
                .entry(summary.version.clone())
                .or_insert(summary);
        }
    }

    Ok(summaries_by_name)
}

#[cfg(test)]
mod tests {
    use crate::runtime::resolver::{Dependency, PackageId, PackageSpecifier, SourceId, SourceKind};

    use super::*;

    #[derive(Debug, Default)]
    struct InMemoryRegistry {
        packages: HashMap<String, HashMap<Version, Vec<Dependency>>>,
    }

    #[async_trait::async_trait]
    impl Registry for InMemoryRegistry {
        async fn query(&self, pkg: &PackageSpecifier) -> Result<Vec<Summary>, Error> {
            let (full_name, version_constraint) = match pkg {
                PackageSpecifier::Registry { full_name, version } => (full_name, version),
                _ => return Ok(Vec::new()),
            };

            let candidates = match self.packages.get(full_name) {
                Some(versions) => versions
                    .iter()
                    .filter(|(v, _)| version_constraint.matches(v)),
                None => return Ok(Vec::new()),
            };

            let summaries = candidates
                .map(|(version, deps)| make_summary(full_name, version, deps))
                .collect();

            Ok(summaries)
        }
    }

    fn make_summary(full_name: &str, version: &Version, deps: &[Dependency]) -> Summary {
        Summary {
            package_name: full_name.to_string(),
            version: version.clone(),
            webc: "https://example.com".parse().unwrap(),
            webc_sha256: [0; 32],
            dependencies: deps.to_vec(),
            commands: Vec::new(),
            entrypoint: None,
            source: dummy_source(),
        }
    }

    fn dummy_source() -> SourceId {
        SourceId::new(
            SourceKind::LocalRegistry,
            "http://localhost".parse().unwrap(),
        )
    }

    macro_rules! resolver_test {
        (
            $( #[$attr:meta] )*
            name = $name:ident,
            roots = [ $root:literal ],
            registry = {
                $(
                    $pkg_name:literal => {
                        $(
                            $pkg_version:literal => {
                                $(
                                    $dep_alias:literal => ($dep_name:literal, $dep_constraint:literal)
                                ),*
                                $(,)?
                            }
                        ),*
                        $(,)?
                    }
                ),*

                $(,)?
            },
            expected_dependency_graph = {
                $(
                    ($expected_name:literal, $expected_version:literal) => {
                        $(
                            $expected_dep_alias:literal => ($expected_dep_name:literal, $expected_dep_version:literal)
                        ),*
                        $(,)?
                    }
                ),*
                $(,)?
            }
            $(,)?
        ) => {
            $( #[$attr] )*
            #[tokio::test]
            #[allow(dead_code, unused_mut)]
            async fn $name() {
                let mut registry = InMemoryRegistry::default();

                $(
                    let versions = registry.packages.entry($pkg_name.to_string())
                        .or_default();
                    $(
                        let version: Version = $pkg_version.parse().unwrap();
                        let deps = vec![
                            $(
                                Dependency {
                                    alias: $dep_alias.to_string(),
                                    pkg: PackageSpecifier::Registry {
                                        full_name: $dep_name.to_string(),
                                        version: $dep_constraint.parse().unwrap(),
                                    }
                                }
                            ),*
                        ];
                        versions.insert(version, deps);
                    )*
                )*

                let (root_name, root_version) = $root.split_once('@').unwrap();
                let root_version: Version = root_version.parse().unwrap();
                let deps = &registry.packages[root_name][&root_version];
                let root = make_summary(root_name, &root_version, deps);

                let resolution = resolve(&root, &registry).await.unwrap();

                let mut expected_dependency_graph: HashMap<PackageId, HashMap<String, PackageId>> = HashMap::new();
                $(
                    let id = PackageId {
                        package_name: $expected_name.to_string(),
                        version: $expected_version.parse().unwrap(),
                        source: dummy_source(),
                    };
                    let mut deps = HashMap::new();
                    $(
                        let dep = PackageId {
                            package_name: $expected_dep_name.to_string(),
                            version: $expected_dep_version.parse().unwrap(),
                            source: dummy_source(),
                        };
                        deps.insert($expected_dep_alias.to_string(), dep);
                    )*
                    expected_dependency_graph.insert(id, deps);
                )*
                assert_eq!(resolution.graph.dependencies, expected_dependency_graph);
            }
        };
    }

    resolver_test! {
        name = simplest_possible_resolution,
        roots = ["wasmer/no-deps@1.0.0"],
        registry = {
            "wasmer/no-deps" => { "1.0.0" => {} },
        },
        expected_dependency_graph = {
            ("wasmer/no-deps", "1.0.0") => {},
        },
    }

    resolver_test! {
        name = single_dependency,
        roots = ["root@1.0.0"],
        registry = {
            "root" => {
                "1.0.0" => {
                    "dep" => ("dep", "1.0.0"),
                }
            },
        },
        expected_dependency_graph = {
            ("root", "1.0.0") => { "dep" => ("dep", "1.0.0") },
            ("dep", "1.0.0") => {},
        },
    }
}
