use futures::future::LocalBoxFuture;

use super::*;

pub static UNSUPPORTED_SNAPSHOT_CAPTURER: UnsupportedSnapshotCapturer =
    UnsupportedSnapshotCapturer {};

/// The default for runtime is to use the unsupported snapshot capturer
/// which will fail to snapshot if one attempts to do so.
#[derive(Debug, Default)]
pub struct UnsupportedSnapshotCapturer {}

impl SnapshotCapturer for UnsupportedSnapshotCapturer {
    fn write<'a>(&'a self, entry: SnapshotLog<'a>) -> LocalBoxFuture<'a, anyhow::Result<()>> {
        tracing::debug!("snapshot event: {:?}", entry);
        Box::pin(async { Err(anyhow::format_err!("unsupported")) })
    }

    fn read<'a>(&'a self) -> LocalBoxFuture<'a, anyhow::Result<Option<SnapshotLog<'a>>>> {
        Box::pin(async { Ok(None) })
    }
}
