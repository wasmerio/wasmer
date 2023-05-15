use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use anyhow::Error;

use crate::runtime::resolver::{
    DependencyGraph, ItemLocation, Registry, Resolution, ResolvedPackage, Summary,
};

/// Given the [`Summary`] for a root package, resolve its dependency graph and
/// figure out how it could be executed.
pub async fn resolve(root: &Summary, registry: &impl Registry) -> Result<Resolution, Error> {
    let graph = resolve_dependency_graph(root, registry).await?;
    let package = resolve_package(&graph)?;

    Ok(Resolution { graph, package })
}

async fn resolve_dependency_graph(
    root: &Summary,
    registry: &impl Registry,
) -> Result<DependencyGraph, Error> {
    let mut dependencies = HashMap::new();
    let mut summaries = HashMap::new();

    summaries.insert(root.package_id(), root.clone());

    let mut to_visit = VecDeque::new();

    to_visit.push_back(root.clone());

    while let Some(summary) = to_visit.pop_front() {
        let mut deps = HashMap::new();

        for dep in &summary.dependencies {
            let dep_summary = registry.latest(&dep.pkg).await?;
            deps.insert(dep.alias().to_string(), dep_summary.package_id());
            summaries.insert(dep_summary.package_id(), dep_summary.clone());
            to_visit.push_back(dep_summary);
        }

        dependencies.insert(summary.package_id(), deps);
    }

    Ok(DependencyGraph {
        root: root.package_id(),
        dependencies,
        summaries,
    })
}

/// Given a [`DependencyGraph`], figure out how the resulting "package" would
/// look when loaded at runtime.
fn resolve_package(dependency_graph: &DependencyGraph) -> Result<ResolvedPackage, Error> {
    // FIXME: This code is all super naive and will break the moment there
    // are any conflicts or duplicate names.

    let mut commands = BTreeMap::new();
    let mut entrypoint = None;
    // TODO: Add filesystem mappings to summary and figure out the final mapping
    // for this dependency graph.
    let filesystem = Vec::new();

    let mut to_check = VecDeque::new();
    let mut visited = HashSet::new();

    to_check.push_back(&dependency_graph.root);

    while let Some(next) = to_check.pop_front() {
        visited.insert(next);
        let summary = &dependency_graph.summaries[next];

        // set the entrypoint, if necessary
        if entrypoint.is_none() {
            if let Some(entry) = &summary.entrypoint {
                entrypoint = Some(entry.clone());
            }
        }

        // Blindly copy across all commands
        for cmd in &summary.commands {
            let resolved = ItemLocation {
                name: cmd.name.clone(),
                package: summary.package_id(),
            };
            commands.insert(cmd.name.clone(), resolved);
        }

        let remaining_dependencies = dependency_graph.dependencies[next]
            .values()
            .filter(|id| !visited.contains(id));
        to_check.extend(remaining_dependencies);
    }

    Ok(ResolvedPackage {
        root_package: dependency_graph.root.clone(),
        commands,
        entrypoint,
        filesystem,
    })
}

#[cfg(test)]
mod tests {
    use semver::Version;

    use crate::runtime::resolver::{PackageId, PackageSpecifier, SourceId, SourceKind};

    use super::*;

    #[derive(Debug, Default)]
    struct InMemoryRegistry {
        packages: HashMap<String, HashMap<Version, Summary>>,
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
                    .filter(|(v, _)| version_constraint.matches(v))
                    .map(|(_, s)| s),
                None => return Ok(Vec::new()),
            };

            Ok(candidates.cloned().collect())
        }
    }

    fn dummy_source() -> SourceId {
        SourceId::new(
            SourceKind::LocalRegistry,
            "http://localhost".parse().unwrap(),
        )
    }

    /// An incremental token muncher that will update fields on a [`Summary`]
    /// object if they are set.
    macro_rules! setup_summary {
        ($summary:expr,
            $(,)?
            dependencies => {
                $(
                    $dep_alias:literal => ($dep_name:literal, $dep_constraint:literal)
                ),*
                $(,)?
            }
            $($rest:tt)*
        ) => {
            $(
                $summary.dependencies.push($crate::runtime::resolver::Dependency {
                    alias: $dep_alias.to_string(),
                    pkg: format!("{}@{}", $dep_name, $dep_constraint).parse().unwrap(),
                });
            )*
        };
        ($summary:expr,
            $(,)?
            commands => [ $($command_name:literal),* $(,)? ]
            $($rest:tt)*
        ) => {
            $(
                $summary.commands.push($crate::runtime::resolver::Command {
                    name: $command_name.to_string(),
                });
            )*
        };
        ($summary:expr $(,)?) => {};
    }

    /// Populate a [`InMemoryRegistry`], using [`setup_summary`] to configure
    /// the [`Summary`] before it is added to the registry.
    macro_rules! setup_registry {
        (
            $(
                ($pkg_name:literal, $pkg_version:literal) => { $($summary:tt)* }
            ),*
            $(,)?
        ) => {{
            let mut registry = InMemoryRegistry::default();

            $(
                let versions = registry.packages.entry($pkg_name.to_string())
                    .or_default();
                let version: Version = $pkg_version.parse().unwrap();
                let mut summary = $crate::runtime::resolver::Summary {
                    package_name: $pkg_name.to_string(),
                    version: version.clone(),
                    webc: format!("https://wapm.io/{}@{}", $pkg_name, $pkg_version).parse().unwrap(),
                    webc_sha256: [0; 32],
                    dependencies: Vec::new(),
                    commands: Vec::new(),
                    entrypoint: None,
                    source: dummy_source(),
                };
                setup_summary!(summary, $($summary)*);
                versions.insert(version, summary);
            )*

            registry
        }};
    }

    /// Shorthand for defining [`DependencyGraph::dependencies`].
    macro_rules! setup_dependency_graph {
        (
            $(
                ($expected_name:literal, $expected_version:literal) => {
                    $(
                        $expected_dep_alias:literal => ($expected_dep_name:literal, $expected_dep_version:literal)
                    ),*
                    $(,)?
                }
            ),*
            $(,)?
        ) => {{
            let mut dependencies: HashMap<PackageId, HashMap<String, PackageId>> = HashMap::new();

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
                dependencies.insert(id, deps);
            )*

            dependencies
        }};
    }

    macro_rules! resolver_test {
        (
            $( #[$attr:meta] )*
            name = $name:ident,
            root = ($root_name:literal, $root_version:literal),
            registry { $($registry:tt)* },
            expected {
                dependency_graph = { $($dependency_graph:tt)* },
                package = $resolved:expr,
            },
        ) => {
            $( #[$attr] )*
            #[tokio::test]
            #[allow(dead_code, unused_mut)]
            async fn $name() {
                let registry = setup_registry!($($registry)*);

                let root_version: Version = $root_version.parse().unwrap();
                let root = registry.packages[$root_name][&root_version].clone();

                let resolution = resolve(&root, &registry).await.unwrap();

                let expected_dependency_graph = setup_dependency_graph!($($dependency_graph)*);
                assert_eq!(
                    resolution.graph.dependencies,
                    expected_dependency_graph,
                    "Incorrect dependency graph",
                );
                let package: ResolvedPackage = $resolved;
                assert_eq!(resolution.package, package);
            }
        };
    }

    macro_rules! map {
        (
            $(
                $key:expr => $value:expr
            ),*
            $(,)?
        ) => {
            vec![
                $( ($key.into(), $value.into()) ),*
            ]
            .into_iter()
            .collect()
        }
    }

    fn pkg_id(name: &str, version: &str) -> PackageId {
        PackageId {
            package_name: name.to_string(),
            version: version.parse().unwrap(),
            source: dummy_source(),
        }
    }

    resolver_test! {
        name = no_deps_and_no_commands,
        root = ("root", "1.0.0"),
        registry {
            ("root", "1.0.0") => { }
        },
        expected {
            dependency_graph = {
                ("root", "1.0.0") => {},
            },
            package = ResolvedPackage {
                root_package: pkg_id("root", "1.0.0"),
                commands: BTreeMap::new(),
                entrypoint: None,
                filesystem: Vec::new(),
            },
        },
    }

    resolver_test! {
        name = no_deps_one_command,
        root = ("root", "1.0.0"),
        registry {
            ("root", "1.0.0") => {
                commands => ["asdf"],
             }
        },
        expected {
            dependency_graph = {
                ("root", "1.0.0") => {},
            },
            package = ResolvedPackage {
                root_package: pkg_id("root", "1.0.0"),
                commands: map! {
                    "asdf" => ItemLocation {
                        name: "asdf".to_string(),
                        package: pkg_id("root", "1.0.0"),
                    },
                },
                entrypoint: None,
                filesystem: Vec::new(),
            },
        },
    }

    resolver_test! {
        name = single_dependency,
        root = ("root", "1.0.0"),
        registry {
            ("root", "1.0.0") => {
                dependencies => {
                    "dep" => ("dep", "=1.0.0"),
                }
            },
            ("dep", "1.0.0") => { },
        },
        expected {
            dependency_graph = {
                ("root", "1.0.0") => { "dep" => ("dep", "1.0.0") },
                ("dep", "1.0.0") => {},
            },
            package = ResolvedPackage {
                root_package: pkg_id("root", "1.0.0"),
                commands: BTreeMap::new(),
                entrypoint: None,
                filesystem: Vec::new(),
            },
        },
    }

    resolver_test! {
        name = linear_dependency_chain,
        root = ("first", "1.0.0"),
        registry {
            ("first", "1.0.0") => {
                dependencies => {
                    "second" => ("second", "=1.0.0"),
                }
            },
            ("second", "1.0.0") => {
                dependencies => {
                    "third" => ("third", "=1.0.0"),
                }
            },
            ("third", "1.0.0") => {},
        },
        expected {
            dependency_graph = {
                ("first", "1.0.0") => { "second" => ("second", "1.0.0") },
                ("second", "1.0.0") => { "third" => ("third", "1.0.0") },
                ("third", "1.0.0") => {},
            },
            package = ResolvedPackage {
                root_package: pkg_id("first", "1.0.0"),
                commands: BTreeMap::new(),
                entrypoint: None,
                filesystem: Vec::new(),
            },
        },
    }

    resolver_test! {
        name = pick_the_latest_dependency_when_multiple_are_possible,
        root = ("root", "1.0.0"),
        registry {
            ("root", "1.0.0") => {
                dependencies => {
                    "dep" => ("dep", "^1.0.0"),
                }
            },
            ("dep", "1.0.0") => {},
            ("dep", "1.0.1") => {},
            ("dep", "1.0.2") => {},
        },
        expected {
            dependency_graph = {
                ("root", "1.0.0") => { "dep" => ("dep", "1.0.2") },
                ("dep", "1.0.2") => {},
            },
            package = ResolvedPackage {
                root_package: pkg_id("root", "1.0.0"),
                commands: BTreeMap::new(),
                entrypoint: None,
                filesystem: Vec::new(),
            },
        },
    }

    resolver_test! {
        #[ignore = "Version merging isn't implemented"]
        name = merge_compatible_versions,
        root = ("root", "1.0.0"),
        registry {
            ("root", "1.0.0") => {
                dependencies => {
                    "first" => ("first", "=1.0.0"),
                    "second" => ("second", "=1.0.0"),
                }
            },
            ("first", "1.0.0") => {
                dependencies => {
                    "common" => ("common", "^1.0.0"),
                }
            },
            ("second", "1.0.0") => {
                dependencies => {
                    "common" => ("common", ">1.1,<1.3"),
                }
            },
            ("common", "1.0.0") => {},
            ("common", "1.1.0") => {},
            ("common", "1.2.0") => {},
            ("common", "1.5.0") => {},
        },
        expected {
            dependency_graph = {
                ("root", "1.0.0") => {
                    "first" => ("first", "1.0.0"),
                    "second" => ("second", "1.0.0"),
                 },
                ("first", "1.0.0") => {
                    "common" => ("common", "1.2.0"),
                },
                ("second", "1.0.0") => {
                    "common" => ("common", "1.2.0"),
                },
                ("common", "1.2.0") => {},
            },
            package = ResolvedPackage {
                root_package: pkg_id("root", "1.0.0"),
                commands: BTreeMap::new(),
                entrypoint: None,
                filesystem: Vec::new(),
            },
        },
    }
}
