use derivative::Derivative;
use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    sync::{Arc, Mutex},
};
use virtual_fs::Fd;

use super::*;

#[derive(Debug)]
struct StateDescriptor {
    events: Vec<usize>,
    write_map: HashMap<MemoryRange, usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct MemoryRange {
    start: u64,
    end: u64,
}
impl From<Range<u64>> for MemoryRange {
    fn from(value: Range<u64>) -> Self {
        Self {
            start: value.start,
            end: value.end,
        }
    }
}

#[derive(Derivative)]
#[derivative(Debug)]
struct State {
    // We maintain a memory map of the events that are significant
    memory_map: HashMap<MemoryRange, usize>,
    // Thread events are only maintained while the thread and the
    // process are still running
    thread_map: HashMap<crate::WasiThreadId, usize>,
    // Any descriptors are assumed to be read only operations until
    // they actually do something that changes the system
    suspect_descriptors: HashMap<Fd, Vec<usize>>,
    // Any descriptors are assumed to be read only operations until
    // they actually do something that changes the system
    keep_descriptors: HashMap<Fd, StateDescriptor>,
    // Everything that will be retained during the next compact
    whitelist: HashSet<usize>,
    // We use an event index to track what to keep
    event_index: usize,
    // The delta list is used for all the events that happened
    // after a compact started
    delta_list: Option<Vec<usize>>,
    // The inner journal that we will write to
    #[derivative(Debug = "ignore")]
    inner_tx: Box<DynWritableJournal>,
    // The inner journal that we read from
    #[derivative(Debug = "ignore")]
    inner_rx: Box<DynReadableJournal>,
}

impl State {
    fn create_filter<J>(&self, inner: J) -> FilteredJournal
    where
        J: Journal,
    {
        let mut filter = FilteredJournal::new(inner)
            .with_filter_events(self.whitelist.clone().into_iter().collect());
        for (_, e) in self.memory_map.iter() {
            filter.add_event_to_whitelist(*e);
        }
        for t in self.thread_map.iter() {
            filter.add_event_to_whitelist(*t.1);
        }
        for (_, d) in self.suspect_descriptors.iter() {
            for e in d.iter() {
                filter.add_event_to_whitelist(*e);
            }
        }
        for (_, d) in self.keep_descriptors.iter() {
            for e in d.events.iter() {
                filter.add_event_to_whitelist(*e);
            }
            for e in d.write_map.values() {
                filter.add_event_to_whitelist(*e);
            }
        }
        filter
    }
}

/// Deduplicates memory and stacks to reduce the number of volume of
/// log events sent to its inner capturer. Compacting the events occurs
/// in line as the events are generated
#[derive(Debug, Clone)]
pub struct CompactingJournalTx {
    state: Arc<Mutex<State>>,
    compacting: Arc<Mutex<()>>,
}

#[derive(Derivative)]
#[derivative(Debug)]
pub struct CompactingJournalRx {
    #[derivative(Debug = "ignore")]
    inner: Box<DynReadableJournal>,
}

#[derive(Debug)]
pub struct CompactingJournal {
    tx: CompactingJournalTx,
    rx: CompactingJournalRx,
}

impl CompactingJournal {
    pub fn new<J>(inner: J) -> anyhow::Result<Self>
    where
        J: Journal,
    {
        let (tx, rx) = inner.split();
        Ok(Self {
            tx: CompactingJournalTx {
                state: Arc::new(Mutex::new(State {
                    inner_tx: tx,
                    inner_rx: rx.as_restarted()?,
                    memory_map: Default::default(),
                    thread_map: Default::default(),
                    suspect_descriptors: Default::default(),
                    keep_descriptors: Default::default(),
                    whitelist: Default::default(),
                    delta_list: None,
                    event_index: 0,
                })),
                compacting: Arc::new(Mutex::new(())),
            },
            rx: CompactingJournalRx { inner: rx },
        })
    }
}

impl CompactingJournalTx {
    pub fn create_filter<J>(&self, inner: J) -> FilteredJournal
    where
        J: Journal,
    {
        let state = self.state.lock().unwrap();
        state.create_filter(inner)
    }

    /// Compacts the inner journal into a new journal
    pub fn compact<J>(&mut self, new_journal: J) -> anyhow::Result<()>
    where
        J: Journal,
    {
        // Enter a compacting lock
        let _guard = self.compacting.lock().unwrap();

        // The first thing we do is create a filter that we
        // place around the new journal so that it only receives new events
        let (new_journal, replay_rx) = {
            let mut state = self.state.lock().unwrap();
            state.delta_list.replace(Default::default());
            (
                state.create_filter(new_journal),
                state.inner_rx.as_restarted()?,
            )
        };

        // Read all the events and feed them into the filtered journal
        while let Some(entry) = replay_rx.read()? {
            new_journal.write(entry)?;
        }

        // We now go into a blocking situation which will freeze the journals
        let mut state = self.state.lock().unwrap();

        // Now we build a filtered journal which will pick up any events that were
        // added which we did the compacting
        let new_journal = FilteredJournal::new(new_journal.into_inner()).with_filter_events(
            state
                .delta_list
                .take()
                .unwrap_or_default()
                .into_iter()
                .collect(),
        );

        // Now we feed all the events into the new journal using the delta filter
        let replay_rx = state.inner_rx.as_restarted()?;
        while let Some(entry) = replay_rx.read()? {
            new_journal.write(entry)?;
        }

        // Now we install the new journal
        let (mut tx, mut rx) = new_journal.into_inner().split();
        std::mem::swap(&mut state.inner_tx, &mut tx);
        std::mem::swap(&mut state.inner_rx, &mut rx);

        Ok(())
    }
}

impl WritableJournal for CompactingJournalTx {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<()> {
        let mut state = self.state.lock().unwrap();
        let event_index = state.event_index;
        state.event_index += 1;

        if let Some(delta) = state.delta_list.as_mut() {
            delta.push(event_index);
        }

        match &entry {
            JournalEntry::UpdateMemoryRegion { region, .. } => {
                state.memory_map.insert(region.clone().into(), event_index);
            }
            JournalEntry::SetThread { id, .. } => {
                state.thread_map.insert(*id, event_index);
            }
            JournalEntry::CloseThread { id, .. } => {
                state.thread_map.remove(&id);
            }
            JournalEntry::ProcessExit { .. } => {
                state.thread_map.clear();
                state.memory_map.clear();
                state.suspect_descriptors.clear();
                state.whitelist.insert(event_index);
            }
            JournalEntry::CloseFileDescriptor { fd } => {
                // If its not suspect we need to record this event
                if state.suspect_descriptors.remove(&fd).is_some() {
                    // suspect descriptors that are closed are dropped
                    // as they made no material difference to the state
                } else if let Some(e) = state.keep_descriptors.get_mut(fd) {
                    e.events.push(event_index);
                } else {
                    state.whitelist.insert(event_index);
                }
            }
            JournalEntry::OpenFileDescriptor { fd, .. } => {
                // All file descriptors are opened in a suspect state which
                // means if they are closed without modifying the file system
                // then the events will be ignored.
                state.suspect_descriptors.insert(*fd, vec![event_index]);
            }
            // We keep non-mutable events for file descriptors that are suspect
            JournalEntry::FileDescriptorSeek { fd, .. } => {
                if let Some(events) = state.suspect_descriptors.get_mut(&fd) {
                    events.push(event_index);
                } else if let Some(s) = state.keep_descriptors.get_mut(&fd) {
                    s.events.push(event_index);
                } else {
                    state.whitelist.insert(event_index);
                }
            }
            // Things that modify a file descriptor mean that it is
            // no longer suspect and thus it needs to be kept
            JournalEntry::FileDescriptorAdvise { fd, .. }
            | JournalEntry::FileDescriptorAllocate { fd, .. }
            | JournalEntry::FileDescriptorSetFlags { fd, .. }
            | JournalEntry::FileDescriptorSetTimes { fd, .. }
            | JournalEntry::FileDescriptorWrite { fd, .. }
            | JournalEntry::DuplicateFileDescriptor {
                original_fd: fd, ..
            } => {
                // Its no longer suspect
                if let Some(events) = state.suspect_descriptors.remove(&fd) {
                    state.keep_descriptors.insert(
                        *fd,
                        StateDescriptor {
                            events,
                            write_map: Default::default(),
                        },
                    );
                }
                if let Some(state) = state.keep_descriptors.get_mut(fd) {
                    if let JournalEntry::FileDescriptorWrite { offset, data, .. } = &entry {
                        state.write_map.insert(
                            MemoryRange {
                                start: *offset,
                                end: *offset + data.len() as u64,
                            },
                            event_index,
                        );
                    } else {
                        state.events.push(event_index);
                    }
                } else {
                    state.whitelist.insert(event_index);
                }
            }
            // Renumbered file descriptors will retain their suspect status
            JournalEntry::RenumberFileDescriptor { old_fd, new_fd } => {
                if let Some(mut events) = state.suspect_descriptors.remove(old_fd) {
                    events.push(event_index);
                    state.suspect_descriptors.insert(*new_fd, events);
                } else if let Some(mut s) = state.keep_descriptors.remove(old_fd) {
                    s.events.push(event_index);
                    state.keep_descriptors.insert(*new_fd, s);
                } else {
                    state.whitelist.insert(event_index);
                }
            }
            _ => {
                // The fallthrough is to whitelist the event so that it will
                // be reflected in the next compaction event
                state.whitelist.insert(event_index);
            }
        }
        state.inner_tx.write(entry)
    }
}

impl CompactingJournal {
    /// Compacts the inner journal into a new journal
    pub fn compact<J>(&mut self, new_journal: J) -> anyhow::Result<()>
    where
        J: Journal,
    {
        self.tx.compact(new_journal)
    }
}

impl ReadableJournal for CompactingJournalRx {
    fn read(&self) -> anyhow::Result<Option<JournalEntry<'_>>> {
        self.inner.read()
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        self.inner.as_restarted()
    }
}

impl WritableJournal for CompactingJournal {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<()> {
        self.tx.write(entry)
    }
}

impl ReadableJournal for CompactingJournal {
    fn read(&self) -> anyhow::Result<Option<JournalEntry<'_>>> {
        self.rx.read()
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        let state = self.tx.state.lock().unwrap();
        state.inner_rx.as_restarted()
    }
}

impl Journal for CompactingJournal {
    fn split(self) -> (Box<DynWritableJournal>, Box<DynReadableJournal>) {
        (Box::new(self.tx), Box::new(self.rx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn run_test<'a>(
        in_records: Vec<JournalEntry<'a>>,
        out_records: Vec<JournalEntry<'a>>,
    ) -> anyhow::Result<()> {
        // Build a journal that will store the records before compacting
        let in_file = tempfile::NamedTempFile::new()?;
        let mut compacting_journal = CompactingJournal::new(LogFileJournal::from_file(
            in_file.as_file().try_clone().unwrap(),
        )?)?;
        for record in in_records {
            compacting_journal.write(record)?;
        }

        // Now we build a new one using the compactor
        let new_file = tempfile::NamedTempFile::new()?;
        let new_journal = LogFileJournal::from_file(new_file.as_file().try_clone()?)?;
        compacting_journal.compact(new_journal)?;

        // Read the records
        let new_records = compacting_journal.as_restarted()?;
        for record1 in out_records {
            let record2 = new_records.read()?;
            assert_eq!(Some(record1), record2);
        }
        assert!(new_records.read()?.is_none());

        Ok(())
    }

    #[tracing_test::traced_test]
    #[test]
    pub fn test_compact_purge_duplicate_memory_writes() {
        run_test(
            vec![
                JournalEntry::UpdateMemoryRegion {
                    region: 0..16,
                    data: [11u8; 16].to_vec().into(),
                },
                JournalEntry::UpdateMemoryRegion {
                    region: 0..16,
                    data: [22u8; 16].to_vec().into(),
                },
            ],
            vec![JournalEntry::UpdateMemoryRegion {
                region: 0..16,
                data: [22u8; 16].to_vec().into(),
            }],
        )
        .unwrap()
    }

    #[tracing_test::traced_test]
    #[test]
    pub fn test_compact_thread_stacks() {
        run_test(
            vec![
                JournalEntry::SetThread {
                    id: 4321.into(),
                    call_stack: [44u8; 87].to_vec().into(),
                    memory_stack: [55u8; 34].to_vec().into(),
                    store_data: [66u8; 70].to_vec().into(),
                    is_64bit: true,
                },
                JournalEntry::SetThread {
                    id: 1234.into(),
                    call_stack: [11u8; 124].to_vec().into(),
                    memory_stack: [22u8; 51].to_vec().into(),
                    store_data: [33u8; 87].to_vec().into(),
                    is_64bit: true,
                },
                JournalEntry::SetThread {
                    id: 65.into(),
                    call_stack: [77u8; 34].to_vec().into(),
                    memory_stack: [88u8; 51].to_vec().into(),
                    store_data: [99u8; 12].to_vec().into(),
                    is_64bit: true,
                },
                JournalEntry::CloseThread {
                    id: 1234.into(),
                    exit_code: None,
                },
            ],
            vec![
                JournalEntry::SetThread {
                    id: 4321.into(),
                    call_stack: [44u8; 87].to_vec().into(),
                    memory_stack: [55u8; 34].to_vec().into(),
                    store_data: [66u8; 70].to_vec().into(),
                    is_64bit: true,
                },
                JournalEntry::SetThread {
                    id: 65.into(),
                    call_stack: [77u8; 34].to_vec().into(),
                    memory_stack: [88u8; 51].to_vec().into(),
                    store_data: [99u8; 12].to_vec().into(),
                    is_64bit: true,
                },
            ],
        )
        .unwrap()
    }

    #[tracing_test::traced_test]
    #[test]
    pub fn test_compact_processed_exited() {
        run_test(
            vec![
                JournalEntry::UpdateMemoryRegion {
                    region: 0..16,
                    data: [11u8; 16].to_vec().into(),
                },
                JournalEntry::SetThread {
                    id: 4321.into(),
                    call_stack: [44u8; 87].to_vec().into(),
                    memory_stack: [55u8; 34].to_vec().into(),
                    store_data: [66u8; 70].to_vec().into(),
                    is_64bit: true,
                },
                JournalEntry::OpenFileDescriptor {
                    fd: 1234,
                    dirfd: 3452345,
                    dirflags: 0,
                    path: "/blah".into(),
                    o_flags: Oflags::all(),
                    fs_rights_base: Rights::all(),
                    fs_rights_inheriting: Rights::all(),
                    fs_flags: Fdflags::all(),
                },
                JournalEntry::ProcessExit { exit_code: None },
            ],
            vec![JournalEntry::ProcessExit { exit_code: None }],
        )
        .unwrap()
    }
}
