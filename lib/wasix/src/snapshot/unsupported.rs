use futures::future::BoxFuture;

use super::*;

pub static UNSUPPORTED_SNAP_SHOOTER: UnsupportedSnapShooter = UnsupportedSnapShooter {};

/// The default for runtime is to use the unsupported snap-shooter
/// which will fail to snapshot if one attempts to do so.
#[derive(Debug, Default)]
pub struct UnsupportedSnapShooter {}

impl SnapShooter for UnsupportedSnapShooter {
    fn write<'a>(&'a self, _entry: SnapshotLog<'a>) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async { Err(anyhow::format_err!("unsupported")) })
    }

    fn read<'a>(&'a self) -> BoxFuture<'a, anyhow::Result<Option<SnapshotLog<'a>>>> {
        Box::pin(async { Ok(None) })
    }
}
