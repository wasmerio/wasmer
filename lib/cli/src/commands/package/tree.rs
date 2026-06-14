use anyhow::{Context, bail};
use wasmer_config::package::{PackageIdent, PackageSource};

use crate::config::WasmerEnv;

#[derive(clap::Parser, Debug)]
pub struct PackageTree {
    #[clap(flatten)]
    pub env: WasmerEnv,

    /// The package whose dependency tree should be shown.
    package: PackageSource,
}

struct DependencyNode {
    label: String,
    dependencies: Vec<DependencyNode>,
}

impl PackageTree {
    pub(crate) fn execute(&self) -> Result<(), anyhow::Error> {
        let (full_name, version) = match &self.package {
            PackageSource::Ident(PackageIdent::Named(id)) => {
                let version = id.version_or_default().to_string();
                let version = if version == "*" {
                    String::from("latest")
                } else {
                    version
                };
                (id.full_name(), version)
            }
            PackageSource::Ident(PackageIdent::Hash(hash)) => {
                bail!(
                    "package tree does not support hashes ({}); \
                     please use a named package identifier (e.g. namespace/name@version)",
                    hash
                )
            }
            PackageSource::Path(path) => {
                bail!("cannot show dependency tree for local path '{path}'")
            }
            PackageSource::Url(url) => {
                bail!("cannot show dependency tree for URL '{url}'")
            }
        };

        let client = self.env.client_unauthennticated()?;
        let rt = tokio::runtime::Runtime::new()?;
        let root = self.fetch_tree(&rt, &client, &full_name, &version, &mut Vec::new())?;

        Self::print_tree(&root);

        Ok(())
    }

    fn fetch_tree(
        &self,
        rt: &tokio::runtime::Runtime,
        client: &wasmer_backend_api::WasmerClient,
        name: &str,
        version: &str,
        visited: &mut Vec<String>,
    ) -> Result<DependencyNode, anyhow::Error> {
        let pkg = rt
            .block_on(
                wasmer_backend_api::query::get_package_versions_with_dependencies(
                    client,
                    name.to_string(),
                    version.to_string(),
                ),
            )?
            .with_context(|| {
                format!(
                    "could not retrieve package '{}@{}' from registry '{}'",
                    name,
                    version,
                    client.graphql_endpoint(),
                )
            })?;

        let ns = pkg.package.namespace.as_deref().unwrap_or("");
        let label = if ns.is_empty() {
            format!("{}@{}", pkg.package.package_name, pkg.version)
        } else {
            format!("{}/{}@{}", ns, pkg.package.package_name, pkg.version)
        };

        if visited.contains(&label) {
            return Ok(DependencyNode {
                label: format!("{} (*)", label),
                dependencies: Vec::new(),
            });
        }

        visited.push(label.clone());

        let dependencies = pkg
            .dependencies
            .edges
            .into_iter()
            .filter_map(|e| e.and_then(|e| e.node))
            .map(|dep| {
                let dep_ns = dep.package.namespace.as_deref().unwrap_or("");
                let dep_name = if dep_ns.is_empty() {
                    dep.package.package_name.clone()
                } else {
                    format!("{}/{}", dep_ns, dep.package.package_name)
                };

                self.fetch_tree(rt, client, &dep_name, &dep.version, visited)
            })
            .collect::<Result<Vec<_>, anyhow::Error>>()?;

        visited.pop();

        Ok(DependencyNode {
            label,
            dependencies,
        })
    }

    fn print_tree(node: &DependencyNode) {
        println!("{}", node.label);
        Self::print_children(&node.dependencies, "");
    }

    fn print_children(deps: &[DependencyNode], prefix: &str) {
        for (i, dep) in deps.iter().enumerate() {
            let is_last = i == deps.len() - 1;
            let connector = "-- ";
            println!("{}{}{}", prefix, connector, dep.label);

            let child_prefix = if is_last {
                format!("{}    ", prefix)
            } else {
                format!("{}|   ", prefix)
            };

            Self::print_children(&dep.dependencies, &child_prefix);
        }
    }
}
