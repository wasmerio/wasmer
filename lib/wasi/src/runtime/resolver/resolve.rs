use anyhow::Error;

use crate::{
    bin_factory::BinaryPackage,
    runtime::resolver::{PackageLoader, Registry, Resolution, RootPackage},
};

pub async fn load_package_tree(
    _loader: &impl PackageLoader,
    _resolution: &Resolution,
) -> Result<BinaryPackage, Error> {
    todo!();
}

/// Given a [`RootPackage`], resolve its dependency graph and figure out
/// how it could be reconstituted.
pub async fn resolve(_root: &RootPackage, _registry: &impl Registry) -> Result<Resolution, Error> {
    todo!();
}
