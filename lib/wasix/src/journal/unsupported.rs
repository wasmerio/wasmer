use futures::future::LocalBoxFuture;

use super::*;

pub static UNSUPPORTED_SNAPSHOT_CAPTURER: UnsupportedJournal = UnsupportedJournal {};

/// The default for runtime is to use the unsupported journal
/// which will fail to write journal entries if one attempts to do so.
#[derive(Debug, Default)]
pub struct UnsupportedJournal {}

impl Journal for UnsupportedJournal {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> LocalBoxFuture<'a, anyhow::Result<()>> {
        tracing::debug!("snapshot event: {:?}", entry);
        Box::pin(async { Err(anyhow::format_err!("unsupported")) })
    }

    fn read(&self) -> LocalBoxFuture<'_, anyhow::Result<Option<JournalEntry<'_>>>> {
        Box::pin(async { Ok(None) })
    }
}
