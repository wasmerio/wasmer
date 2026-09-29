use comfy_table::Table;
use wasmer_backend_api::WasmerClient;

use crate::commands::AsyncCliCommand;
use crate::utils::render::CliRender;

pub mod credentials;
pub mod enable_s3;
pub mod list;

/// App volume management.
#[derive(Debug, clap::Parser)]
pub enum CmdAppVolumes {
    Credentials(credentials::CmdAppVolumesCredentials),
    List(list::CmdAppVolumesList),
    RotateSecrets(credentials::rotate_secrets::CmdAppVolumesRotateSecrets),
    EnableS3(enable_s3::CmdAppVolumesEnableS3),
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdAppVolumes {
    type Output = ();

    async fn run_async(self) -> Result<Self::Output, anyhow::Error> {
        match self {
            Self::Credentials(c) => {
                c.run_async().await?;
                Ok(())
            }
            Self::RotateSecrets(c) => {
                c.run_async().await?;
                Ok(())
            }
            Self::List(c) => {
                c.run_async().await?;
                Ok(())
            }
            Self::EnableS3(c) => {
                c.run_async().await?;
                Ok(())
            }
        }
    }
}

/// A volume of an app: the single view every `wasmer app volume` subcommand
/// operates on.
///
/// It unifies the persistent `AppVolume` node (the manageable entity with id, mount
/// path, S3 state and credentials) with the active version's descriptor
/// (friendly name, used size), joined by mount path. Volumes not declared in the
/// active version (e.g. created via the dashboard) simply have no `name`/
/// `used_size`.
pub(crate) struct Volume {
    pub id: wasmer_backend_api::types::Id,
    pub volume_id: String,
    pub mount_path: String,
    pub s3_enabled: bool,
    pub s3: Option<wasmer_backend_api::types::S3>,
    pub name: Option<String>,
    /// Used size in bytes, from the active version descriptor.
    pub used_size: Option<i64>,
}

impl Volume {
    /// A human-readable label for status messages and error lists.
    pub fn label(&self) -> String {
        match &self.name {
            Some(name) => format!("{name} ({})", self.mount_path),
            None => self.mount_path.clone(),
        }
    }
}

/// List the app's volumes: the persistent `AppVolume` nodes enriched with the
/// friendly name and used size from the active version's descriptors, joined by
/// mount path.
pub(crate) async fn list_volumes(
    client: &WasmerClient,
    owner: &str,
    app_name: &str,
) -> Result<Vec<Volume>, anyhow::Error> {
    let persistent =
        wasmer_backend_api::query::get_deploy_app_volumes(client, owner, app_name).await?;

    // The active-version descriptors carry the friendly name and used size.
    // They're best-effort decoration, so don't fail the command if unavailable.
    let version = wasmer_backend_api::query::get_app_volumes(client, owner, app_name)
        .await
        .unwrap_or_default();

    let mut meta_by_mount: std::collections::HashMap<String, (String, Option<i64>)> =
        std::collections::HashMap::new();
    for descriptor in version {
        let used_size = descriptor.used_size.map(|b| b.0);
        for mount in descriptor.mount_paths {
            meta_by_mount
                .entry(mount.path.trim_end_matches('/').to_string())
                .or_insert_with(|| (descriptor.name.clone(), used_size));
        }
    }

    Ok(persistent
        .into_iter()
        .map(|volume| {
            let meta = meta_by_mount.get(volume.mount_path.trim_end_matches('/'));
            Volume {
                id: volume.id,
                volume_id: volume.volume_id,
                mount_path: volume.mount_path,
                s3_enabled: volume.s3_enabled,
                s3: volume.s3,
                name: meta.map(|(name, _)| name.clone()),
                used_size: meta.and_then(|(_, used)| *used),
            }
        })
        .collect())
}

/// Resolve a `--volume` selector to a single volume. The selector may be the
/// friendly name, the mount path (exactly as shown by `wasmer app volume list`,
/// e.g. `/data`), or the volume id.
///
/// The volume id is opaque and unique, so an exact id match is unambiguous and
/// always wins: a volume can never be shadowed by another volume's (user-chosen)
/// name or mount path. If a volume's name collides with a volume's id, the id
/// always wins.
pub(crate) fn select_volume_by_selector<'a>(
    app_name: &str,
    volumes: &'a [Volume],
    selector: &str,
) -> Result<&'a Volume, anyhow::Error> {
    if let Some(volume) = volumes.iter().find(|v| v.volume_id == selector) {
        return Ok(volume);
    }

    let matched: Vec<&Volume> = volumes
        .iter()
        .filter(|v| v.name.as_deref() == Some(selector) || v.mount_path == selector)
        .collect();

    match matched.as_slice() {
        [] => {
            let available = volumes
                .iter()
                .map(Volume::label)
                .collect::<Vec<_>>()
                .join(", ");
            if available.is_empty() {
                anyhow::bail!("App {app_name} has no volumes.");
            }
            anyhow::bail!(
                "App {app_name} has no volume matching '{selector}'. Available volumes: {available}"
            );
        }
        [volume] => Ok(volume),
        multiple => {
            let colliding = multiple
                .iter()
                .map(|v| format!("{} [id {}]", v.label(), v.volume_id))
                .collect::<Vec<_>>()
                .join(", ");
            anyhow::bail!(
                "'{selector}' matches multiple volumes of app {app_name}: {colliding}. \
                 Pass the volume id to select one."
            )
        }
    }
}

/// The `wasmer app volume list` row for a [`Volume`].
#[derive(serde::Serialize)]
pub(crate) struct VolumeListItem {
    pub name: Option<String>,
    pub mount_path: String,
    pub volume_id: String,
    pub used_size: Option<i64>,
    pub s3_enabled: bool,
}

impl From<&Volume> for VolumeListItem {
    fn from(volume: &Volume) -> Self {
        Self {
            name: volume.name.clone(),
            mount_path: volume.mount_path.clone(),
            volume_id: volume.volume_id.clone(),
            used_size: volume.used_size,
            s3_enabled: volume.s3_enabled,
        }
    }
}

impl VolumeListItem {
    fn row(&self) -> Vec<String> {
        vec![
            self.name.clone().unwrap_or_else(|| "-".to_string()),
            self.mount_path.clone(),
            crate::types::format_disk_size_opt(
                self.used_size.map(wasmer_backend_api::types::BigInt),
            ),
            if self.s3_enabled {
                "enabled"
            } else {
                "disabled"
            }
            .to_string(),
        ]
    }
}

impl CliRender for VolumeListItem {
    fn render_item_table(&self) -> String {
        let row = self.row();
        let mut table = Table::new();
        table.add_rows([
            vec!["Name".to_string(), row[0].clone()],
            vec!["Mount path".to_string(), row[1].clone()],
            vec!["Used size".to_string(), row[2].clone()],
            vec!["S3".to_string(), row[3].clone()],
        ]);
        table.to_string()
    }

    fn render_list_table(items: &[Self]) -> String {
        let mut table = Table::new();
        table.set_header(vec!["Name", "Mount path", "Used size", "S3"]);
        table.add_rows(items.iter().map(|item| item.row()));
        table.to_string()
    }
}
