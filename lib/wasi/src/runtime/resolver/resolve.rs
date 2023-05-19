use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use crate::runtime::resolver::{
    DependencyGraph, ItemLocation, PackageId, PackageInfo, PackageSummary, Registry, Resolution,
    ResolvedPackage,
};

use super::FileSystemMapping;

/// Given the [`PackageInfo`] for a root package, resolve its dependency graph
/// and figure out how it could be executed.
#[tracing::instrument(level = "debug", skip_all)]
pub async fn resolve(
    root_id: &PackageId,
    root: &PackageInfo,
    registry: &dyn Registry,
) -> Result<Resolution, ResolveError> {
    let graph = resolve_dependency_graph(root_id, root, registry).await?;
    let package = resolve_package(&graph)?;

    Ok(Resolution { graph, package })
}

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error(transparent)]
    Registry(anyhow::Error),
    #[error("Dependency cycle detected: {}", print_cycle(_0))]
    Cycle(Vec<PackageId>),
}

impl ResolveError {
    pub fn as_cycle(&self) -> Option<&[PackageId]> {
        match self {
            ResolveError::Cycle(cycle) => Some(cycle),
            ResolveError::Registry(_) => None,
        }
    }
}

fn print_cycle(packages: &[PackageId]) -> String {
    packages
        .iter()
        .map(|pkg_id| {
            let PackageId {
                package_name,
                version,
                ..
            } = pkg_id;
            format!("{package_name}@{version}")
        })
        .collect::<Vec<_>>()
        .join(" → ")
}

async fn resolve_dependency_graph(
    root_id: &PackageId,
    root: &PackageInfo,
    registry: &dyn Registry,
) -> Result<DependencyGraph, ResolveError> {
    let mut dependencies = HashMap::new();
    let mut package_info = HashMap::new();
    let mut distribution = HashMap::new();

    package_info.insert(root_id.clone(), root.clone());

    let mut to_visit = VecDeque::new();

    to_visit.push_back((root_id.clone(), root.clone()));

    while let Some((id, info)) = to_visit.pop_front() {
        let mut deps = HashMap::new();

        for dep in &info.dependencies {
            let dep_summary = registry
                .latest(&dep.pkg)
                .await
                .map_err(ResolveError::Registry)?;
            deps.insert(dep.alias().to_string(), dep_summary.package_id());
            let dep_id = dep_summary.package_id();

            if dependencies.contains_key(&dep_id) {
                // We don't need to visit this dependency again
                continue;
            }

            let PackageSummary { pkg, dist } = dep_summary;

            to_visit.push_back((dep_id.clone(), pkg.clone()));
            package_info.insert(dep_id.clone(), pkg);
            distribution.insert(dep_id, dist);
        }

        dependencies.insert(id, deps);
    }

    check_for_cycles(&dependencies, root_id)?;

    Ok(DependencyGraph {
        root: root_id.clone(),
        dependencies,
        package_info,
        distribution,
    })
}

/// Check for dependency cycles by doing a Depth First Search of the graph,
/// starting at the root.
fn check_for_cycles(
    dependencies: &HashMap<PackageId, HashMap<String, PackageId>>,
    root: &PackageId,
) -> Result<(), ResolveError> {
    fn search<'a>(
        dependencies: &'a HashMap<PackageId, HashMap<String, PackageId>>,
        id: &'a PackageId,
        visited: &mut HashSet<&'a PackageId>,
        stack: &mut Vec<&'a PackageId>,
    ) -> Result<(), ResolveError> {
        if let Some(index) = stack.iter().position(|item| *item == id) {
            // we've detected a cycle!
            let mut cycle: Vec<_> = stack.drain(index..).cloned().collect();
            cycle.push(id.clone());
            return Err(ResolveError::Cycle(cycle));
        }

        if visited.contains(&id) {
            // We already know this dependency is fine
            return Ok(());
        }

        stack.push(id);
        for dep in dependencies[id].values() {
            search(dependencies, dep, visited, stack)?;
        }
        stack.pop();

        Ok(())
    }

    let mut visited = HashSet::new();
    let mut stack = Vec::new();

    search(dependencies, root, &mut visited, &mut stack)
}

/// Given a [`DependencyGraph`], figure out how the resulting "package" would
/// look when loaded at runtime.
fn resolve_package(dependency_graph: &DependencyGraph) -> Result<ResolvedPackage, ResolveError> {
    // FIXME: This code is all super naive and will break the moment there
    // are any conflicts or duplicate names.
    tracing::trace!("Resolving the package");

    let mut commands = BTreeMap::new();

    let filesystem = resolve_filesystem_mapping(dependency_graph)?;

    let mut to_check = VecDeque::new();
    let mut visited = HashSet::new();

    to_check.push_back(&dependency_graph.root);

    let mut entrypoint = dependency_graph.root_info().entrypoint.clone();

    while let Some(next) = to_check.pop_front() {
        visited.insert(next);
        let pkg = &dependency_graph.package_info[next];

        // set the entrypoint, if necessary
        if entrypoint.is_none() {
            if let Some(entry) = &pkg.entrypoint {
                tracing::trace!(
                    entrypoint = entry.as_str(),
                    parent.name=next.package_name.as_str(),
                    parent.version=%next.version,
                    "Inheriting the entrypoint",
                );

                entrypoint = Some(entry.clone());
            }
        }

        // Blindly copy across all commands
        for cmd in &pkg.commands {
            let resolved = ItemLocation {
                name: cmd.name.clone(),
                package: next.clone(),
            };
            tracing::trace!(
                command.name=cmd.name.as_str(),
                pkg.name=next.package_name.as_str(),
                pkg.version=%next.version,
                "Discovered command",
            );
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

fn resolve_filesystem_mapping(
    _dependency_graph: &DependencyGraph,
) -> Result<Vec<FileSystemMapping>, ResolveError> {
    // TODO: Add filesystem mappings to summary and figure out the final mapping
    // for this dependency graph.
    // See <https://github.com/wasmerio/wasmer/issues/3744> for more.
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use crate::runtime::resolver::{
        inputs::{DistributionInfo, PackageInfo},
        Dependency, InMemorySource, MultiSourceRegistry, PackageSpecifier,
    };

    use super::*;

    struct RegistryBuilder(InMemorySource);

    impl RegistryBuilder {
        fn new() -> Self {
            RegistryBuilder(InMemorySource::new())
        }

        fn register(&mut self, name: &str, version: &str) -> AddPackageVersion<'_> {
            let pkg = PackageInfo {
                name: name.to_string(),
                version: version.parse().unwrap(),
                dependencies: Vec::new(),
                commands: Vec::new(),
                entrypoint: None,
            };
            let dist = DistributionInfo {
                webc: format!("http://localhost/{name}@{version}")
                    .parse()
                    .unwrap(),
                webc_sha256: [0; 32].into(),
            };
            let summary = PackageSummary { pkg, dist };

            AddPackageVersion {
                builder: &mut self.0,
                summary,
            }
        }

        fn finish(&self) -> MultiSourceRegistry {
            let mut registry = MultiSourceRegistry::new();
            registry.add_source(self.0.clone());
            registry
        }

        fn get(&self, package: &str, version: &str) -> &PackageSummary {
            let version = version.parse().unwrap();
            self.0.get(package, &version).unwrap()
        }

        fn start_dependency_graph(&self) -> DependencyGraphBuilder<'_> {
            DependencyGraphBuilder {
                dependencies: HashMap::new(),
                source: &self.0,
            }
        }
    }

    #[derive(Debug)]
    struct AddPackageVersion<'builder> {
        builder: &'builder mut InMemorySource,
        summary: PackageSummary,
    }

    impl<'builder> AddPackageVersion<'builder> {
        fn with_dependency(&mut self, name: &str, version_constraint: &str) -> &mut Self {
            self.with_aliased_dependency(name, name, version_constraint)
        }

        fn with_aliased_dependency(
            &mut self,
            alias: &str,
            name: &str,
            version_constraint: &str,
        ) -> &mut Self {
            let pkg = PackageSpecifier::Registry {
                full_name: name.to_string(),
                version: version_constraint.parse().unwrap(),
            };

            self.summary.pkg.dependencies.push(Dependency {
                alias: alias.to_string(),
                pkg,
            });

            self
        }

        fn with_command(&mut self, name: &str) -> &mut Self {
            self.summary
                .pkg
                .commands
                .push(crate::runtime::resolver::Command {
                    name: name.to_string(),
                });
            self
        }

        fn with_entrypoint(&mut self, name: &str) -> &mut Self {
            self.summary.pkg.entrypoint = Some(name.to_string());
            self
        }
    }

    impl<'builder> Drop for AddPackageVersion<'builder> {
        fn drop(&mut self) {
            let summary = self.summary.clone();
            self.builder.add(summary);
        }
    }

    #[derive(Debug)]
    struct DependencyGraphBuilder<'source> {
        dependencies: HashMap<PackageId, HashMap<String, PackageId>>,
        source: &'source InMemorySource,
    }

    impl<'source> DependencyGraphBuilder<'source> {
        fn insert(
            &mut self,
            package: &str,
            version: &str,
        ) -> DependencyGraphEntryBuilder<'source, '_> {
            let version = version.parse().unwrap();
            let pkg_id = self.source.get(package, &version).unwrap().package_id();
            DependencyGraphEntryBuilder {
                builder: self,
                pkg_id,
                dependencies: HashMap::new(),
            }
        }

        fn finish(self) -> HashMap<PackageId, HashMap<String, PackageId>> {
            self.dependencies
        }
    }

    #[derive(Debug)]
    struct DependencyGraphEntryBuilder<'source, 'builder> {
        builder: &'builder mut DependencyGraphBuilder<'source>,
        pkg_id: PackageId,
        dependencies: HashMap<String, PackageId>,
    }

    impl<'source, 'builder> DependencyGraphEntryBuilder<'source, 'builder> {
        fn with_dependency(&mut self, name: &str, version: &str) -> &mut Self {
            self.with_aliased_dependency(name, name, version)
        }

        fn with_aliased_dependency(&mut self, alias: &str, name: &str, version: &str) -> &mut Self {
            let version = version.parse().unwrap();
            let dep_id = self
                .builder
                .source
                .get(name, &version)
                .unwrap()
                .package_id();
            self.dependencies.insert(alias.to_string(), dep_id);
            self
        }
    }

    impl<'source, 'builder> Drop for DependencyGraphEntryBuilder<'source, 'builder> {
        fn drop(&mut self) {
            self.builder
                .dependencies
                .insert(self.pkg_id.clone(), self.dependencies.clone());
        }
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

    #[tokio::test]
    async fn no_deps_and_no_commands() {
        let mut builder = RegistryBuilder::new();
        builder.register("root", "1.0.0");
        let registry = builder.finish();
        let root = builder.get("root", "1.0.0");

        let resolution = resolve(&root.package_id(), &root.pkg, &registry)
            .await
            .unwrap();

        let mut dependency_graph = builder.start_dependency_graph();
        dependency_graph.insert("root", "1.0.0");
        assert_eq!(resolution.graph.dependencies, dependency_graph.finish());
        assert_eq!(
            resolution.package,
            ResolvedPackage {
                root_package: root.package_id(),
                commands: BTreeMap::new(),
                entrypoint: None,
                filesystem: Vec::new(),
            }
        );
    }

    #[tokio::test]
    async fn no_deps_one_command() {
        let mut builder = RegistryBuilder::new();
        builder.register("root", "1.0.0").with_command("asdf");
        let registry = builder.finish();
        let root = builder.get("root", "1.0.0");

        let resolution = resolve(&root.package_id(), &root.pkg, &registry)
            .await
            .unwrap();

        let mut dependency_graph = builder.start_dependency_graph();
        dependency_graph.insert("root", "1.0.0");
        assert_eq!(resolution.graph.dependencies, dependency_graph.finish());
        assert_eq!(
            resolution.package,
            ResolvedPackage {
                root_package: root.package_id(),
                commands: map! {
                    "asdf" => ItemLocation {
                        name: "asdf".to_string(),
                        package: root.package_id(),
                    },
                },
                entrypoint: None,
                filesystem: Vec::new(),
            }
        );
    }

    #[tokio::test]
    async fn single_dependency() {
        let mut builder = RegistryBuilder::new();
        builder
            .register("root", "1.0.0")
            .with_dependency("dep", "=1.0.0");
        builder.register("dep", "1.0.0");
        let registry = builder.finish();
        let root = builder.get("root", "1.0.0");

        let resolution = resolve(&root.package_id(), &root.pkg, &registry)
            .await
            .unwrap();

        let mut dependency_graph = builder.start_dependency_graph();
        dependency_graph
            .insert("root", "1.0.0")
            .with_dependency("dep", "1.0.0");
        dependency_graph.insert("dep", "1.0.0");
        assert_eq!(resolution.graph.dependencies, dependency_graph.finish());
        assert_eq!(
            resolution.package,
            ResolvedPackage {
                root_package: root.package_id(),
                commands: BTreeMap::new(),
                entrypoint: None,
                filesystem: Vec::new(),
            }
        );
    }

    #[tokio::test]
    async fn linear_dependency_chain() {
        let mut builder = RegistryBuilder::new();
        builder
            .register("first", "1.0.0")
            .with_dependency("second", "=1.0.0");
        builder
            .register("second", "1.0.0")
            .with_dependency("third", "=1.0.0");
        builder.register("third", "1.0.0");
        let registry = builder.finish();
        let root = builder.get("first", "1.0.0");

        let resolution = resolve(&root.package_id(), &root.pkg, &registry)
            .await
            .unwrap();

        let mut dependency_graph = builder.start_dependency_graph();
        dependency_graph
            .insert("first", "1.0.0")
            .with_dependency("second", "1.0.0");
        dependency_graph
            .insert("second", "1.0.0")
            .with_dependency("third", "1.0.0");
        dependency_graph.insert("third", "1.0.0");
        assert_eq!(resolution.graph.dependencies, dependency_graph.finish());
        assert_eq!(
            resolution.package,
            ResolvedPackage {
                root_package: root.package_id(),
                commands: BTreeMap::new(),
                entrypoint: None,
                filesystem: Vec::new(),
            }
        );
    }

    #[tokio::test]
    async fn pick_the_latest_dependency_when_multiple_are_possible() {
        let mut builder = RegistryBuilder::new();
        builder
            .register("root", "1.0.0")
            .with_dependency("dep", "^1.0.0");
        builder.register("dep", "1.0.0");
        builder.register("dep", "1.0.1");
        builder.register("dep", "1.0.2");
        let registry = builder.finish();
        let root = builder.get("root", "1.0.0");

        let resolution = resolve(&root.package_id(), &root.pkg, &registry)
            .await
            .unwrap();

        let mut dependency_graph = builder.start_dependency_graph();
        dependency_graph
            .insert("root", "1.0.0")
            .with_dependency("dep", "1.0.2");
        dependency_graph.insert("dep", "1.0.2");
        assert_eq!(resolution.graph.dependencies, dependency_graph.finish());
        assert_eq!(
            resolution.package,
            ResolvedPackage {
                root_package: root.package_id(),
                commands: BTreeMap::new(),
                entrypoint: None,
                filesystem: Vec::new(),
            }
        );
    }

    #[tokio::test]
    #[ignore = "Version merging isn't implemented"]
    async fn merge_compatible_versions() {
        let mut builder = RegistryBuilder::new();
        builder
            .register("root", "1.0.0")
            .with_dependency("first", "=1.0.0")
            .with_dependency("second", "=1.0.0");
        builder
            .register("first", "1.0.0")
            .with_dependency("common", "^1.0.0");
        builder
            .register("second", "1.0.0")
            .with_dependency("common", ">1.1,<1.3");
        builder.register("common", "1.0.0");
        builder.register("common", "1.1.0");
        builder.register("common", "1.2.0");
        builder.register("common", "1.5.0");
        let registry = builder.finish();
        let root = builder.get("root", "1.0.0");

        let resolution = resolve(&root.package_id(), &root.pkg, &registry)
            .await
            .unwrap();

        let mut dependency_graph = builder.start_dependency_graph();
        dependency_graph
            .insert("root", "1.0.0")
            .with_dependency("first", "1.0.0")
            .with_dependency("second", "1.0.0");
        dependency_graph
            .insert("first", "1.0.0")
            .with_dependency("common", "1.2.0");
        dependency_graph
            .insert("second", "1.0.0")
            .with_dependency("common", "1.2.0");
        dependency_graph.insert("common", "1.2.0");
        assert_eq!(resolution.graph.dependencies, dependency_graph.finish());
        assert_eq!(
            resolution.package,
            ResolvedPackage {
                root_package: root.package_id(),
                commands: BTreeMap::new(),
                entrypoint: None,
                filesystem: Vec::new(),
            }
        );
    }

    #[tokio::test]
    async fn commands_from_dependencies_end_up_in_the_package() {
        let mut builder = RegistryBuilder::new();
        builder
            .register("root", "1.0.0")
            .with_dependency("first", "=1.0.0")
            .with_dependency("second", "=1.0.0");
        builder
            .register("first", "1.0.0")
            .with_command("first-command");
        builder
            .register("second", "1.0.0")
            .with_command("second-command");
        let registry = builder.finish();
        let root = builder.get("root", "1.0.0");

        let resolution = resolve(&root.package_id(), &root.pkg, &registry)
            .await
            .unwrap();

        let mut dependency_graph = builder.start_dependency_graph();
        dependency_graph
            .insert("root", "1.0.0")
            .with_dependency("first", "1.0.0")
            .with_dependency("second", "1.0.0");
        dependency_graph.insert("first", "1.0.0");
        dependency_graph.insert("second", "1.0.0");
        assert_eq!(resolution.graph.dependencies, dependency_graph.finish());
        assert_eq!(
            resolution.package,
            ResolvedPackage {
                root_package: root.package_id(),
                commands: map! {
                    "first-command" => ItemLocation {
                        name: "first-command".to_string(),
                        package: builder.get("first", "1.0.0").package_id(),
                     },
                    "second-command" => ItemLocation {
                        name: "second-command".to_string(),
                        package: builder.get("second", "1.0.0").package_id(),
                     },
                },
                entrypoint: None,
                filesystem: Vec::new(),
            }
        );
    }

    #[tokio::test]
    #[ignore = "TODO: Re-order the way commands are resolved"]
    async fn commands_in_root_shadow_their_dependencies() {
        let mut builder = RegistryBuilder::new();
        builder
            .register("root", "1.0.0")
            .with_dependency("dep", "=1.0.0")
            .with_command("command");
        builder.register("dep", "1.0.0").with_command("command");
        let registry = builder.finish();
        let root = builder.get("root", "1.0.0");

        let resolution = resolve(&root.package_id(), &root.pkg, &registry)
            .await
            .unwrap();

        let mut dependency_graph = builder.start_dependency_graph();
        dependency_graph
            .insert("root", "1.0.0")
            .with_dependency("dep", "1.0.0");
        dependency_graph.insert("dep", "1.0.0");
        assert_eq!(resolution.graph.dependencies, dependency_graph.finish());
        assert_eq!(
            resolution.package,
            ResolvedPackage {
                root_package: root.package_id(),
                commands: map! {
                    "command" => ItemLocation {
                        name: "command".to_string(),
                        package: builder.get("root", "1.0.0").package_id(),
                     },
                },
                entrypoint: None,
                filesystem: Vec::new(),
            }
        );
    }

    #[tokio::test]
    async fn cyclic_dependencies() {
        let mut builder = RegistryBuilder::new();
        builder
            .register("root", "1.0.0")
            .with_dependency("dep", "=1.0.0");
        builder
            .register("dep", "1.0.0")
            .with_dependency("root", "=1.0.0");
        let registry = builder.finish();
        let root = builder.get("root", "1.0.0");

        let err = resolve(&root.package_id(), &root.pkg, &registry)
            .await
            .unwrap_err();

        let cycle = err.as_cycle().unwrap().to_vec();
        assert_eq!(
            cycle,
            [
                builder.get("root", "1.0.0").package_id(),
                builder.get("dep", "1.0.0").package_id(),
                builder.get("root", "1.0.0").package_id(),
            ]
        );
    }

    #[tokio::test]
    async fn entrypoint_is_inherited() {
        let mut builder = RegistryBuilder::new();
        builder
            .register("root", "1.0.0")
            .with_dependency("dep", "=1.0.0");
        builder
            .register("dep", "1.0.0")
            .with_command("entry")
            .with_entrypoint("entry");
        let registry = builder.finish();
        let root = builder.get("root", "1.0.0");

        let resolution = resolve(&root.package_id(), &root.pkg, &registry)
            .await
            .unwrap();

        assert_eq!(
            resolution.package,
            ResolvedPackage {
                root_package: root.package_id(),
                commands: map! {
                    "entry" => ItemLocation {
                        name: "entry".to_string(),
                        package: builder.get("dep", "1.0.0").package_id(),
                     },
                },
                entrypoint: Some("entry".to_string()),
                filesystem: Vec::new(),
            }
        );
    }

    #[test]
    fn cyclic_error_message() {
        let cycle = [
            PackageId {
                package_name: "root".to_string(),
                version: "1.0.0".parse().unwrap(),
            },
            PackageId {
                package_name: "dep".to_string(),
                version: "1.0.0".parse().unwrap(),
            },
            PackageId {
                package_name: "root".to_string(),
                version: "1.0.0".parse().unwrap(),
            },
        ];

        let message = print_cycle(&cycle);

        assert_eq!(message, "root@1.0.0 → dep@1.0.0 → root@1.0.0");
    }
}
