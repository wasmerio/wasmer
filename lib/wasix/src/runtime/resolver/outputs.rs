use std::{collections::BTreeMap, ops::Index, path::PathBuf};

use petgraph::{
    graph::{DiGraph, NodeIndex},
    visit::EdgeRef,
};
use wasmer_config::package::PackageId;

use crate::runtime::resolver::{DistributionInfo, PackageInfo};

#[derive(Debug, Clone)]
pub struct Resolution {
    pub package: ResolvedPackage,
    pub graph: DependencyGraph,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemLocation {
    /// The item's original name.
    pub name: String,
    /// The package this item comes from.
    pub package: PackageId,
}

/// An acyclic, directed dependency graph.
#[derive(Debug, Clone)]
pub struct DependencyGraph {
    root: NodeIndex,
    graph: DiGraph<Node, Edge>,
    packages: BTreeMap<PackageId, NodeIndex>,
}

impl DependencyGraph {
    pub(crate) fn new(
        root: NodeIndex,
        graph: DiGraph<Node, Edge>,
        packages: BTreeMap<PackageId, NodeIndex>,
    ) -> Self {
        if cfg!(debug_assertions) {
            // Note: We assume the packages table correctly maps PackageIds to
            // node indices as part of the PartialEq implementation.
            for (id, index) in &packages {
                let node = &graph[*index];
                assert_eq!(*id, node.id, "Mismatch for node {index:?}");
            }
        }
        debug_assert!(
            packages.values().any(|ix| *ix == root),
            "The packages mapping doesn't contain the root node"
        );

        DependencyGraph {
            root,
            graph,
            packages,
        }
    }

    pub fn root_info(&self) -> &PackageInfo {
        let Node { pkg, .. } = &self.graph[self.root];
        pkg
    }

    pub fn id(&self) -> &PackageId {
        let Node { id, .. } = &self.graph[self.root];
        id
    }

    pub fn root(&self) -> NodeIndex {
        self.root
    }

    pub fn graph(&self) -> &DiGraph<Node, Edge> {
        &self.graph
    }

    /// Get a mapping from [`PackageId`]s to [`NodeIndex`]s.
    pub fn packages(&self) -> &BTreeMap<PackageId, NodeIndex> {
        &self.packages
    }

    /// Get an iterator over all the packages in this dependency graph and their
    /// dependency mappings.
    pub fn iter_dependencies(
        &self,
    ) -> impl Iterator<Item = (&'_ PackageId, BTreeMap<&'_ str, &'_ PackageId>)> + '_ {
        self.packages.iter().map(move |(id, index)| {
            let dependencies: BTreeMap<_, _> = self
                .graph
                .edges(*index)
                .map(|edge_ref| {
                    (
                        edge_ref.weight().alias.as_str(),
                        &self.graph[edge_ref.target()].id,
                    )
                })
                .collect();
            (id, dependencies)
        })
    }

    /// Visualise this graph as a DOT program.
    pub fn visualise(&self) -> String {
        let graph = self.graph.map(|_, node| &node.id, |_, edge| &edge.alias);
        petgraph::dot::Dot::new(&graph).to_string()
    }
}

impl Index<NodeIndex> for DependencyGraph {
    type Output = Node;

    #[track_caller]
    fn index(&self, index: NodeIndex) -> &Self::Output {
        &self.graph[index]
    }
}

impl Index<&NodeIndex> for DependencyGraph {
    type Output = Node;

    #[track_caller]
    fn index(&self, index: &NodeIndex) -> &Self::Output {
        &self[*index]
    }
}

impl Index<&PackageId> for DependencyGraph {
    type Output = Node;

    #[track_caller]
    fn index(&self, index: &PackageId) -> &Self::Output {
        let index = self.packages[index];
        &self[index]
    }
}

impl PartialEq for DependencyGraph {
    fn eq(&self, other: &Self) -> bool {
        let DependencyGraph {
            root,
            graph,
            packages,
        } = self;

        // Make sure their roots are the same package
        let this_root = graph.node_weight(*root);
        let other_root = other.graph.node_weight(other.root);

        match (this_root, other_root) {
            (Some(lhs), Some(rhs)) if lhs == rhs => {}
            _ => return false,
        }

        // the packages table *should* just be an optimisation. We've checked
        // it is valid as part of DependencyGraph::new() and the entire graph
        // is immutable, so it's fine to ignore.
        let _ = packages;

        // Most importantly, the graphs should be "the same" (i.e. if a node
        // in one graph is a
        // nodes are connected to the same nodes in both)
        petgraph::algo::is_isomorphic_matching(graph, &other.graph, Node::eq, Edge::eq)
    }
}

impl Eq for DependencyGraph {}

/// A node in the [`DependencyGraph`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub id: PackageId,
    pub pkg: PackageInfo,
    /// Information about how the package is distributed.
    ///
    /// This will only ever be missing for the root package.
    pub dist: Option<DistributionInfo>,
}

/// An edge in the [`DependencyGraph`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    /// The name used by the package when referring to this dependency.
    pub alias: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedFileSystemMapping {
    // TODO: Change this to a new type that isn't coupled to the OS
    pub mount_path: PathBuf,
    pub volume_name: String,
    pub original_path: Option<String>,
    pub package: PackageId,
}

/// A package that has been resolved, but is not yet runnable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPackage {
    pub root_package: PackageId,
    pub commands: BTreeMap<String, ItemLocation>,
    pub entrypoint: Option<String>,
    /// A mapping from paths to the volumes that should be mounted there.
    /// Note: mappings at the start of the list obscure mappings at the end of the list
    /// thus this list represents an inheritance tree
    pub filesystem: Vec<ResolvedFileSystemMapping>,
}
