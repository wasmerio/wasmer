use std::sync::Arc;

use semver::Version;

use crate::runtime::module_cache::ModuleHash;

use super::DynJournal;

/// Describes the journal to be loaded or created
#[derive(Debug, Clone)]
pub struct JournalDescriptor {
    /// Name of the package that holds the module (i.e. `wasmer/wapm2pirita`)
    pub package_name: String,
    /// Version of the package that contains the module (i.e. `1.0``)
    pub version: Version,
    /// Hash of the module that will use this journal
    pub module_hash: ModuleHash,
}

/// The journal factory creates or loads journals based of a set of
/// properties that describe the journal.
pub trait JournalFactory {
    /// Creates or loads a journal based on a descriptor that makes it unique
    ///
    /// It is the responsibility of the implementor of this function
    /// to reuse and reload existing journals based on the descriptor data
    /// that is supplied.
    ///
    /// The factory can return more than one journal where only the last
    /// journal is the active one while the former journals are base journals
    fn load_or_create(&self, desc: JournalDescriptor) -> anyhow::Result<Vec<Arc<DynJournal>>>;
}

pub type DynJournalFactory = dyn JournalFactory + Send + Sync;
