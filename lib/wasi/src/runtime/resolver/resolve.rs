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
    use crate::runtime::resolver::{
        Dependency, InMemorySource, MultiSourceRegistry, PackageId, PackageSpecifier, Source,
    };

    use super::*;

    struct RegistryBuilder(InMemorySource);

    impl RegistryBuilder {
        fn new() -> Self {
            RegistryBuilder(InMemorySource::new())
        }

        fn register(&mut self, name: &str, version: &str) -> AddPackageVersion<'_> {
            let summary = Summary {
                package_name: name.to_string(),
                version: version.parse().unwrap(),
                webc: format!("http://localhost/{name}@{version}")
                    .parse()
                    .unwrap(),
                webc_sha256: [0; 32],
                dependencies: Vec::new(),
                commands: Vec::new(),
                entrypoint: None,
                source: self.0.id(),
            };

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

        fn get(&self, package: &str, version: &str) -> &Summary {
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
        summary: Summary,
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

            self.summary.dependencies.push(Dependency {
                alias: alias.to_string(),
                pkg,
            });

            self
        }

        fn with_command(&mut self, name: &str) -> &mut Self {
            self.summary
                .commands
                .push(crate::runtime::resolver::Command {
                    name: name.to_string(),
                });
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

        let resolution = resolve(root, &registry).await.unwrap();

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

        let resolution = resolve(root, &registry).await.unwrap();

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

        let resolution = resolve(root, &registry).await.unwrap();

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

        let resolution = resolve(root, &registry).await.unwrap();

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

        let resolution = resolve(root, &registry).await.unwrap();

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

        let resolution = resolve(root, &registry).await.unwrap();

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
}
