use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use super::*;

#[derive(Debug)]
struct State {
    on_n_records: Option<u64>,
    on_n_size: Option<u64>,
    on_factor_size: Option<f32>,
    on_drop: bool,
    cnt_records: u64,
    cnt_size: u64,
    ref_size: u64,
}

#[derive(Debug)]
pub struct CompactingLogFileJournal {
    tx: CompactingLogFileJournalTx,
    rx: CompactingLogFileJournalRx,
}

#[derive(Debug)]
pub struct CompactingLogFileJournalTx {
    state: Arc<Mutex<State>>,
    inner: CompactingJournalTx,
    main_path: PathBuf,
    temp_path: PathBuf,
}

#[derive(Debug)]
pub struct CompactingLogFileJournalRx {
    #[allow(dead_code)]
    state: Arc<Mutex<State>>,
    inner: CompactingJournalRx,
}

impl CompactingLogFileJournalRx {
    pub fn swap_inner(&mut self, with: Box<DynReadableJournal>) -> Box<DynReadableJournal> {
        self.inner.swap_inner(with)
    }
}

impl CompactingLogFileJournal {
    pub fn new(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        // We prepare a compacting journal which does nothing
        // with the events other than learn from them
        let counting = CountingJournal::default();
        let mut compacting = CompactingJournal::new(counting.clone())?;

        // We first feed all the entries into the compactor so that
        // it learns all the records
        let log_file = LogFileJournal::new(path.as_ref())?;
        copy_journal(&log_file, &compacting)?;

        // Now everything is learned its time to attach the
        // log file to the compacting journal
        compacting.replace_inner(log_file);
        let (tx, rx) = compacting.into_split();

        let mut temp_filename = path
            .as_ref()
            .file_name()
            .ok_or_else(|| {
                anyhow::format_err!(
                    "The path is not a valid filename - {}",
                    path.as_ref().to_string_lossy()
                )
            })?
            .to_string_lossy()
            .to_string();
        temp_filename.insert_str(0, ".compacting.");
        let temp_path = path.as_ref().with_file_name(&temp_filename);

        let state = Arc::new(Mutex::new(State {
            on_drop: false,
            on_n_records: None,
            on_n_size: None,
            on_factor_size: None,
            cnt_records: 0,
            cnt_size: 0,
            ref_size: counting.size(),
        }));
        let tx = CompactingLogFileJournalTx {
            state: state.clone(),
            inner: tx,
            main_path: path.as_ref().to_path_buf(),
            temp_path,
        };
        let rx = CompactingLogFileJournalRx { state, inner: rx };

        Ok(Self { tx, rx })
    }

    pub fn compact_now(&mut self) -> anyhow::Result<CompactResult> {
        let (result, new_rx) = self.tx.compact_now()?;
        self.rx.inner = new_rx;
        Ok(result)
    }

    pub fn with_compact_on_drop(self) -> Self {
        self.tx.state.lock().unwrap().on_drop = true;
        self
    }

    pub fn with_compact_on_n_records(self, n_records: u64) -> Self {
        self.tx
            .state
            .lock()
            .unwrap()
            .on_n_records
            .replace(n_records);
        self
    }

    pub fn with_compact_on_n_size(self, n_size: u64) -> Self {
        self.tx.state.lock().unwrap().on_n_size.replace(n_size);
        self
    }

    pub fn with_compact_on_factor_size(self, factor_size: f32) -> Self {
        self.tx
            .state
            .lock()
            .unwrap()
            .on_factor_size
            .replace(factor_size);
        self
    }
}

impl CompactingLogFileJournalTx {
    pub fn compact_now(&self) -> anyhow::Result<(CompactResult, CompactingJournalRx)> {
        // Reset the counters
        self.reset_counters();

        // Create the staging file and open it
        std::fs::remove_file(&self.temp_path).ok();
        let target = LogFileJournal::new(self.temp_path.clone())?;

        // Compact the data into the new target and rename it over the last one
        let result = self.inner.compact_to(target)?;
        std::fs::rename(&self.temp_path, &self.main_path)?;

        // Renaming the file has quite a detrimental effect on the file as
        // it means any new mmap operations will fail, hence we need to
        // reopen the log file, seek to the end and reattach it
        let target = LogFileJournal::new(self.main_path.clone())?;

        // We prepare a compacting journal which does nothing
        // with the events other than learn from them
        let counting = CountingJournal::default();
        let mut compacting = CompactingJournal::new(counting)?;
        copy_journal(&target, &compacting)?;

        // Now everything is learned its time to attach the log file to the compacting journal
        // and replace the current one
        compacting.replace_inner(target);
        let (tx, rx) = compacting.into_split();
        self.inner.swap(tx);

        // We take a new reference point for the size of the journal
        {
            let mut state = self.state.lock().unwrap();
            state.ref_size = result.total_size;
        }

        Ok((result, rx))
    }

    pub fn reset_counters(&self) {
        let mut state = self.state.lock().unwrap();
        state.cnt_records = 0;
        state.cnt_size = 0;
    }
}

impl Drop for CompactingLogFileJournalTx {
    fn drop(&mut self) {
        let triggered = self.state.lock().unwrap().on_drop;
        if triggered {
            if let Err(err) = self.compact_now() {
                tracing::error!("failed to compact log - {}", err);
            }
        }
    }
}

impl ReadableJournal for CompactingLogFileJournalRx {
    fn read(&self) -> anyhow::Result<Option<LogReadResult<'_>>> {
        self.inner.read()
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        self.inner.as_restarted()
    }
}

impl WritableJournal for CompactingLogFileJournalTx {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<LogWriteResult> {
        let res = self.inner.write(entry)?;

        let triggered = {
            let mut state = self.state.lock().unwrap();
            if res.record_size() > 0 {
                state.cnt_records += 1;
                state.cnt_size += res.record_size();
            }

            let mut triggered = false;
            if let Some(on) = state.on_n_records.as_ref() {
                if state.cnt_records >= *on {
                    triggered = true;
                }
            }
            if let Some(on) = state.on_n_size.as_ref() {
                if state.cnt_size >= *on {
                    triggered = true;
                }
            }

            if let Some(factor) = state.on_factor_size.as_ref() {
                let next_ref = (*factor * state.ref_size as f32) as u64;
                if state.cnt_size > next_ref {
                    triggered = true;
                }
            }

            triggered
        };

        if triggered {
            self.compact_now()?;
        }

        Ok(res)
    }
}

impl ReadableJournal for CompactingLogFileJournal {
    fn read(&self) -> anyhow::Result<Option<LogReadResult<'_>>> {
        self.rx.read()
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        self.rx.as_restarted()
    }
}

impl WritableJournal for CompactingLogFileJournal {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<LogWriteResult> {
        self.tx.write(entry)
    }
}

impl Journal for CompactingLogFileJournal {
    fn split(self) -> (Box<DynWritableJournal>, Box<DynReadableJournal>) {
        (Box::new(self.tx), Box::new(self.rx))
    }
}
