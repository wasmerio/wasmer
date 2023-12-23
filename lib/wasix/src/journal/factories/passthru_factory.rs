use std::sync::Arc;

use crate::journal::{DynJournal, JournalFactory};

#[derive(Clone)]
pub struct PassthruJournalFactory {
    journals: Vec<Arc<DynJournal>>,
}

impl PassthruJournalFactory {
    pub fn new(journals: Vec<Arc<DynJournal>>) -> Self {
        Self { journals }
    }
}

impl JournalFactory for PassthruJournalFactory {
    fn load_or_create(
        &self,
        _desc: crate::journal::JournalDescriptor,
    ) -> anyhow::Result<Vec<Arc<DynJournal>>> {
        Ok(self.journals.clone())
    }
}
