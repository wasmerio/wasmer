//! Yank packages from the Wasmer registry.

use wasmer_backend_api::WasmerClient;

pub use wasmer_backend_api::types::YankedPackageVersion;

/// Options for [`yank_package_versions`].
#[derive(Debug, Clone, Default)]
pub struct YankOptions {
    /// The exact `PackageVersion` node ids to act on. All must belong to the
    /// same package.
    pub version_ids: Vec<wasmer_backend_api::types::Id>,
    /// Why the versions were yanked. Shown to anyone still pinning them.
    pub reason: Option<String>,
    /// Unyank the given versions when set.
    pub undo: bool,
}

/// Yank (or unyank) an explicit set of package versions by node id.
///
/// Requires package-admin rights. Returns only the versions whose yank state
/// changed, so re-yanking an already-yanked version comes back empty.
pub async fn yank_package_versions(
    client: &WasmerClient,
    opts: YankOptions,
) -> Result<Vec<YankedPackageVersion>, anyhow::Error> {
    let YankOptions {
        version_ids,
        reason,
        undo,
    } = opts;

    wasmer_backend_api::query::yank_package_versions(client, version_ids, reason.as_deref(), undo)
        .await
}
