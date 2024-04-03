use derivative::Derivative;
use std::{
    collections::{HashMap, HashSet},
    ops::{DerefMut, Range},
    sync::{Arc, Mutex},
};
use wasmer_wasix_types::wasi;

use super::*;

pub type Fd = u32;

#[derive(Debug, Default)]
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

#[derive(Debug, Clone, Copy, PartialOrd, Ord, PartialEq, Eq, Hash)]
struct DescriptorLookup(u64);

#[derive(Derivative)]
#[derivative(Debug)]
struct State {
    /// The descriptor seed is used generate descriptor lookups
    descriptor_seed: u64,
    // We maintain a memory map of the events that are significant
    memory_map: HashMap<MemoryRange, usize>,
    // List of all the snapshots
    snapshots: Vec<usize>,
    // Last tty event thats been set
    tty: Option<usize>,
    // Events that create a particular directory
    create_directory: HashMap<String, usize>,
    // Events that remove a particular directory
    remove_directory: HashMap<String, usize>,
    // When creating and truncating a file we have a special
    // lookup so that duplicates can be erased
    create_trunc_file: HashMap<String, Fd>,
    // Thread events are only maintained while the thread and the
    // process are still running
    thread_map: HashMap<u32, usize>,
    // Any descriptors are assumed to be read only operations until
    // they actually do something that changes the system
    suspect_descriptors: HashMap<Fd, DescriptorLookup>,
    // Any descriptors are assumed to be read only operations until
    // they actually do something that changes the system
    keep_descriptors: HashMap<Fd, DescriptorLookup>,
    // We put the IO related to stdio into a special list
    // which can be purged when the program exits as its no longer
    // important.
    stdio_descriptors: HashMap<Fd, DescriptorLookup>,
    // We abstract the descriptor state so that multiple file descriptors
    // can refer to the same file descriptors
    descriptors: HashMap<DescriptorLookup, StateDescriptor>,
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
        let has_threads = !self.thread_map.is_empty();

        let mut filter = FilteredJournalBuilder::new()
            .with_filter_events(self.whitelist.clone().into_iter().collect());
        if let Some(tty) = self.tty.as_ref() {
            filter.add_event_to_whitelist(*tty);
        }
        for e in self.snapshots.iter() {
            filter.add_event_to_whitelist(*e);
        }
        for (_, e) in self.memory_map.iter() {
            filter.add_event_to_whitelist(*e);
        }
        for t in self.thread_map.iter() {
            filter.add_event_to_whitelist(*t.1);
        }
        for (_, e) in self.create_directory.iter() {
            filter.add_event_to_whitelist(*e);
        }
        for (_, e) in self.remove_directory.iter() {
            filter.add_event_to_whitelist(*e);
        }
        for (_, l) in self
            .suspect_descriptors
            .iter()
            .chain(self.keep_descriptors.iter())
        {
            if let Some(d) = self.descriptors.get(l) {
                for e in d.events.iter() {
                    filter.add_event_to_whitelist(*e);
                }
                for e in d.write_map.values() {
                    filter.add_event_to_whitelist(*e);
                }
            }
        }
        if has_threads {
            for (_, l) in self.stdio_descriptors.iter() {
                if let Some(d) = self.descriptors.get(l) {
                    for e in d.events.iter() {
                        filter.add_event_to_whitelist(*e);
                    }
                    for e in d.write_map.values() {
                        filter.add_event_to_whitelist(*e);
                    }
                }
            }
        }
        filter.build(inner)
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

impl CompactingJournalRx {
    pub fn swap_inner(&mut self, mut with: Box<DynReadableJournal>) -> Box<DynReadableJournal> {
        std::mem::swap(&mut self.inner, &mut with);
        with
    }
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
        let state = State {
            inner_tx: tx,
            inner_rx: rx.as_restarted()?,
            tty: None,
            snapshots: Default::default(),
            memory_map: Default::default(),
            thread_map: Default::default(),
            create_directory: Default::default(),
            remove_directory: Default::default(),
            create_trunc_file: Default::default(),
            suspect_descriptors: Default::default(),
            keep_descriptors: Default::default(),
            stdio_descriptors: Default::default(),
            descriptor_seed: 0,
            descriptors: Default::default(),
            whitelist: Default::default(),
            delta_list: None,
            event_index: 0,
        };
        Ok(Self {
            tx: CompactingJournalTx {
                state: Arc::new(Mutex::new(state)),
                compacting: Arc::new(Mutex::new(())),
            },
            rx: CompactingJournalRx { inner: rx },
        })
    }
}

/// Represents the results of a compaction operation
#[derive(Debug, Default)]
pub struct CompactResult {
    pub total_size: u64,
    pub total_events: usize,
}

impl CompactingJournalTx {
    pub fn create_filter<J>(&self, inner: J) -> FilteredJournal
    where
        J: Journal,
    {
        let state = self.state.lock().unwrap();
        state.create_filter(inner)
    }

    pub fn swap(&self, other: Self) -> Self {
        let mut state1 = self.state.lock().unwrap();
        let mut state2 = other.state.lock().unwrap();
        std::mem::swap(state1.deref_mut(), state2.deref_mut());
        drop(state1);
        drop(state2);
        other
    }

    /// Compacts the inner journal into a new journal
    pub fn compact_to<J>(&self, new_journal: J) -> anyhow::Result<CompactResult>
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

        let mut result = CompactResult::default();

        // Read all the events and feed them into the filtered journal and then
        // strip off the filter so that its a normal journal again
        while let Some(entry) = replay_rx.read()? {
            let res = new_journal.write(entry.into_inner())?;
            if res.record_size() > 0 {
                result.total_size += res.record_size();
                result.total_events += 1;
            }
        }
        let new_journal = new_journal.into_inner();

        // We now go into a blocking situation which will freeze the journals
        let mut state = self.state.lock().unwrap();

        // Now we build a filtered journal which will pick up any events that were
        // added which we did the compacting.
        let new_journal = FilteredJournalBuilder::new()
            .with_filter_events(
                state
                    .delta_list
                    .take()
                    .unwrap_or_default()
                    .into_iter()
                    .collect(),
            )
            .build(new_journal);

        // Now we feed all the events into the new journal using the delta filter. After the
        // extra events are added we strip off the filter again
        let replay_rx = state.inner_rx.as_restarted()?;
        while let Some(entry) = replay_rx.read()? {
            new_journal.write(entry.into_inner())?;
        }
        let new_journal = new_journal.into_inner();

        // Now we install the new journal
        let (mut tx, mut rx) = new_journal.split();
        std::mem::swap(&mut state.inner_tx, &mut tx);
        std::mem::swap(&mut state.inner_rx, &mut rx);

        Ok(result)
    }

    pub fn replace_inner<J: Journal>(&self, inner: J) {
        let mut state = self.state.lock().unwrap();
        let (mut tx, mut rx) = inner.split();
        std::mem::swap(&mut state.inner_tx, &mut tx);
        std::mem::swap(&mut state.inner_rx, &mut rx);
    }
}

impl WritableJournal for CompactingJournalTx {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<LogWriteResult> {
        let mut state = self.state.lock().unwrap();
        let event_index = state.event_index;
        state.event_index += 1;

        if let Some(delta) = state.delta_list.as_mut() {
            delta.push(event_index);
        }

        match &entry {
            JournalEntry::UpdateMemoryRegionV1 { region, .. } => {
                state.memory_map.insert(region.clone().into(), event_index);
            }
            JournalEntry::SetThreadV1 { id, .. } => {
                state.thread_map.insert(*id, event_index);
            }
            JournalEntry::CloseThreadV1 { id, .. } => {
                state.thread_map.remove(id);
            }
            JournalEntry::SnapshotV1 { .. } => {
                state.snapshots.push(event_index);
            }
            JournalEntry::ProcessExitV1 { .. } => {
                state.thread_map.clear();
                state.memory_map.clear();
                for (_, lookup) in state.suspect_descriptors.clone() {
                    state.descriptors.remove(&lookup);
                }
                state.suspect_descriptors.clear();
                for (_, lookup) in state.stdio_descriptors.clone() {
                    state.descriptors.remove(&lookup);
                }
                state.stdio_descriptors.clear();
                state.whitelist.insert(event_index);
                state.snapshots.clear();
            }
            JournalEntry::TtySetV1 { .. } => {
                state.tty.replace(event_index);
            }
            JournalEntry::OpenFileDescriptorV1 {
                fd, o_flags, path, ..
            } => {
                // All file descriptors are opened in a suspect state which
                // means if they are closed without modifying the file system
                // then the events will be ignored.
                let lookup = DescriptorLookup(state.descriptor_seed);
                state.descriptor_seed += 1;
                state.suspect_descriptors.insert(*fd, lookup);

                // There is an exception to the rule which is if the create
                // flag is specified its always recorded as a mutating operation
                // because it may create a file that does not exist on the file system
                if o_flags.contains(wasi::Oflags::CREATE) {
                    if let Some(lookup) = state.suspect_descriptors.remove(fd) {
                        state.keep_descriptors.insert(*fd, lookup);
                    }
                }

                // The event itself must be recorded in a staging area
                state
                    .descriptors
                    .entry(lookup)
                    .or_default()
                    .events
                    .push(event_index);

                // Creating a file and erasing anything that was there before means
                // the entire create branch that exists before this one can be ignored
                if o_flags.contains(wasi::Oflags::CREATE) && o_flags.contains(wasi::Oflags::TRUNC) {
                    let path = path.to_string();
                    if let Some(existing) = state.create_trunc_file.remove(&path) {
                        state.suspect_descriptors.remove(&existing);
                        state.keep_descriptors.remove(&existing);
                    }
                    state.create_trunc_file.insert(path, *fd);
                }
            }
            // We keep non-mutable events for file descriptors that are suspect
            JournalEntry::FileDescriptorSeekV1 { fd, .. }
            | JournalEntry::CloseFileDescriptorV1 { fd } => {
                // Get the lookup
                // (if its suspect then it will remove the entry and
                //  thus the entire branch of events it represents is discarded)
                let lookup = if matches!(&entry, JournalEntry::CloseFileDescriptorV1 { .. }) {
                    state.suspect_descriptors.remove(fd)
                } else {
                    state.suspect_descriptors.get(fd).cloned()
                };
                let lookup = lookup
                    .or_else(|| state.keep_descriptors.get(fd).cloned())
                    .or_else(|| state.stdio_descriptors.get(fd).cloned());

                if let Some(lookup) = lookup {
                    let state = state.descriptors.entry(lookup).or_default();
                    state.events.push(event_index);
                } else {
                    state.whitelist.insert(event_index);
                }
            }
            // Things that modify a file descriptor mean that it is
            // no longer suspect and thus it needs to be kept
            JournalEntry::FileDescriptorAdviseV1 { fd, .. }
            | JournalEntry::FileDescriptorAllocateV1 { fd, .. }
            | JournalEntry::FileDescriptorSetFlagsV1 { fd, .. }
            | JournalEntry::FileDescriptorSetTimesV1 { fd, .. }
            | JournalEntry::FileDescriptorWriteV1 { fd, .. } => {
                // Its no longer suspect
                if let Some(lookup) = state.suspect_descriptors.remove(fd) {
                    state.keep_descriptors.insert(*fd, lookup);
                }

                // Get the lookup
                let lookup = state
                    .suspect_descriptors
                    .get(fd)
                    .cloned()
                    .or_else(|| state.keep_descriptors.get(fd).cloned())
                    .or_else(|| state.stdio_descriptors.get(fd).cloned());

                // Update the state
                if let Some(lookup) = lookup {
                    let state = state.descriptors.entry(lookup).or_default();
                    if let JournalEntry::FileDescriptorWriteV1 { offset, data, .. } = &entry {
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
            // Duplicating the file descriptor
            JournalEntry::DuplicateFileDescriptorV1 {
                original_fd,
                copied_fd,
            } => {
                if let Some(lookup) = state.suspect_descriptors.remove(original_fd) {
                    state.suspect_descriptors.insert(*copied_fd, lookup);
                } else if let Some(lookup) = state.keep_descriptors.remove(original_fd) {
                    state.keep_descriptors.insert(*copied_fd, lookup);
                } else if let Some(lookup) = state.stdio_descriptors.remove(original_fd) {
                    state.stdio_descriptors.insert(*copied_fd, lookup);
                } else {
                    state.whitelist.insert(event_index);
                }
            }
            // Renumbered file descriptors will retain their suspect status
            JournalEntry::RenumberFileDescriptorV1 { old_fd, new_fd } => {
                if let Some(lookup) = state.suspect_descriptors.remove(old_fd) {
                    state.suspect_descriptors.insert(*new_fd, lookup);
                } else if let Some(lookup) = state.keep_descriptors.remove(old_fd) {
                    state.keep_descriptors.insert(*new_fd, lookup);
                } else if let Some(lookup) = state.stdio_descriptors.remove(old_fd) {
                    state.stdio_descriptors.insert(*new_fd, lookup);
                } else {
                    state.whitelist.insert(event_index);
                }
            }
            // Creating a new directory only needs to be done once
            JournalEntry::CreateDirectoryV1 { path, .. } => {
                let path = path.to_string();
                state.remove_directory.remove(&path);
                state.create_directory.entry(path).or_insert(event_index);
            }
            // Deleting a directory only needs to be done once
            JournalEntry::RemoveDirectoryV1 { path, .. } => {
                let path = path.to_string();
                state.create_directory.remove(&path);
                state.remove_directory.entry(path).or_insert(event_index);
            }
            _ => {
                // The fallthrough is to whitelist the event so that it will
                // be reflected in the next compaction event
                state.whitelist.insert(event_index);
            }
        }
        state.inner_tx.write(entry)
    }

    fn flush(&self) -> anyhow::Result<()> {
        self.state.lock().unwrap().inner_tx.flush()
    }
}

impl CompactingJournal {
    /// Compacts the inner journal into a new journal
    pub fn compact_to<J>(&mut self, new_journal: J) -> anyhow::Result<CompactResult>
    where
        J: Journal,
    {
        self.tx.compact_to(new_journal)
    }

    pub fn into_split(self) -> (CompactingJournalTx, CompactingJournalRx) {
        (self.tx, self.rx)
    }

    pub fn replace_inner<J: Journal>(&mut self, inner: J) {
        let (inner_tx, inner_rx) = inner.split();
        let inner_rx_restarted = inner_rx.as_restarted().unwrap();

        self.tx
            .replace_inner(RecombinedJournal::new(inner_tx, inner_rx));
        self.rx.inner = inner_rx_restarted;
    }
}

impl ReadableJournal for CompactingJournalRx {
    fn read(&self) -> anyhow::Result<Option<LogReadResult<'_>>> {
        self.inner.read()
    }

    fn as_restarted(&self) -> anyhow::Result<Box<DynReadableJournal>> {
        self.inner.as_restarted()
    }
}

impl WritableJournal for CompactingJournal {
    fn write<'a>(&'a self, entry: JournalEntry<'a>) -> anyhow::Result<LogWriteResult> {
        self.tx.write(entry)
    }

    fn flush(&self) -> anyhow::Result<()> {
        self.tx.flush()
    }
}

impl ReadableJournal for CompactingJournal {
    fn read(&self) -> anyhow::Result<Option<LogReadResult<'_>>> {
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

#[cfg(feature = "journal")]
#[cfg(test)]
mod tests {
    use super::*;

    use wasmer_wasix_types::wasi::Tty;

    pub fn run_test<'a>(
        in_records: Vec<JournalEntry<'a>>,
        out_records: Vec<JournalEntry<'a>>,
    ) -> anyhow::Result<()> {
        // Build a journal that will store the records before compacting
        let mut compacting_journal = CompactingJournal::new(BufferedJournal::default())?;
        for record in in_records {
            compacting_journal.write(record)?;
        }

        // Now we build a new one using the compactor
        let new_journal = BufferedJournal::default();
        compacting_journal.compact_to(new_journal)?;

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
                JournalEntry::UpdateMemoryRegionV1 {
                    region: 0..16,
                    data: [11u8; 16].to_vec().into(),
                },
                JournalEntry::UpdateMemoryRegionV1 {
                    region: 0..16,
                    data: [22u8; 16].to_vec().into(),
                },
            ],
            vec![JournalEntry::UpdateMemoryRegionV1 {
                region: 0..16,
                data: [22u8; 16].to_vec().into(),
            }],
        )
        .unwrap()
    }

    #[tracing_test::traced_test]
    #[test]
    pub fn test_compact_keep_overlapping_memory() {
        run_test(
            vec![
                JournalEntry::UpdateMemoryRegionV1 {
                    region: 0..16,
                    data: [11u8; 16].to_vec().into(),
                },
                JournalEntry::UpdateMemoryRegionV1 {
                    region: 20..36,
                    data: [22u8; 16].to_vec().into(),
                },
            ],
            vec![
                JournalEntry::UpdateMemoryRegionV1 {
                    region: 0..16,
                    data: [11u8; 16].to_vec().into(),
                },
                JournalEntry::UpdateMemoryRegionV1 {
                    region: 20..36,
                    data: [22u8; 16].to_vec().into(),
                },
            ],
        )
        .unwrap()
    }

    #[tracing_test::traced_test]
    #[test]
    pub fn test_compact_keep_adjacent_memory_writes() {
        run_test(
            vec![
                JournalEntry::UpdateMemoryRegionV1 {
                    region: 0..16,
                    data: [11u8; 16].to_vec().into(),
                },
                JournalEntry::UpdateMemoryRegionV1 {
                    region: 16..32,
                    data: [22u8; 16].to_vec().into(),
                },
            ],
            vec![
                JournalEntry::UpdateMemoryRegionV1 {
                    region: 0..16,
                    data: [11u8; 16].to_vec().into(),
                },
                JournalEntry::UpdateMemoryRegionV1 {
                    region: 16..32,
                    data: [22u8; 16].to_vec().into(),
                },
            ],
        )
        .unwrap()
    }

    #[tracing_test::traced_test]
    #[test]
    pub fn test_compact_purge_identical_memory_writes() {
        run_test(
            vec![
                JournalEntry::UpdateMemoryRegionV1 {
                    region: 0..16,
                    data: [11u8; 16].to_vec().into(),
                },
                JournalEntry::UpdateMemoryRegionV1 {
                    region: 0..16,
                    data: [11u8; 16].to_vec().into(),
                },
            ],
            vec![JournalEntry::UpdateMemoryRegionV1 {
                region: 0..16,
                data: [11u8; 16].to_vec().into(),
            }],
        )
        .unwrap()
    }

    #[tracing_test::traced_test]
    #[test]
    pub fn test_compact_thread_stacks() {
        run_test(
            vec![
                JournalEntry::SetThreadV1 {
                    id: 4321.into(),
                    call_stack: [44u8; 87].to_vec().into(),
                    memory_stack: [55u8; 34].to_vec().into(),
                    store_data: [66u8; 70].to_vec().into(),
                    is_64bit: true,
                },
                JournalEntry::SetThreadV1 {
                    id: 1234.into(),
                    call_stack: [11u8; 124].to_vec().into(),
                    memory_stack: [22u8; 51].to_vec().into(),
                    store_data: [33u8; 87].to_vec().into(),
                    is_64bit: true,
                },
                JournalEntry::SetThreadV1 {
                    id: 65.into(),
                    call_stack: [77u8; 34].to_vec().into(),
                    memory_stack: [88u8; 51].to_vec().into(),
                    store_data: [99u8; 12].to_vec().into(),
                    is_64bit: true,
                },
                JournalEntry::CloseThreadV1 {
                    id: 1234.into(),
                    exit_code: None,
                },
            ],
            vec![
                JournalEntry::SetThreadV1 {
                    id: 4321.into(),
                    call_stack: [44u8; 87].to_vec().into(),
                    memory_stack: [55u8; 34].to_vec().into(),
                    store_data: [66u8; 70].to_vec().into(),
                    is_64bit: true,
                },
                JournalEntry::SetThreadV1 {
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
                JournalEntry::UpdateMemoryRegionV1 {
                    region: 0..16,
                    data: [11u8; 16].to_vec().into(),
                },
                JournalEntry::SetThreadV1 {
                    id: 4321.into(),
                    call_stack: [44u8; 87].to_vec().into(),
                    memory_stack: [55u8; 34].to_vec().into(),
                    store_data: [66u8; 70].to_vec().into(),
                    is_64bit: true,
                },
                JournalEntry::SnapshotV1 {
                    when: SystemTime::now(),
                    trigger: SnapshotTrigger::FirstListen,
                },
                JournalEntry::OpenFileDescriptorV1 {
                    fd: 1234,
                    dirfd: 3452345,
                    dirflags: 0,
                    path: "/blah".into(),
                    o_flags: wasi::Oflags::empty(),
                    fs_rights_base: wasi::Rights::all(),
                    fs_rights_inheriting: wasi::Rights::all(),
                    fs_flags: wasi::Fdflags::all(),
                },
                JournalEntry::ProcessExitV1 { exit_code: None },
            ],
            vec![JournalEntry::ProcessExitV1 { exit_code: None }],
        )
        .unwrap()
    }

    #[tracing_test::traced_test]
    #[test]
    pub fn test_compact_file_system_partial_write_survives() {
        run_test(
            vec![
                JournalEntry::OpenFileDescriptorV1 {
                    fd: 1234,
                    dirfd: 3452345,
                    dirflags: 0,
                    path: "/blah".into(),
                    o_flags: wasi::Oflags::empty(),
                    fs_rights_base: wasi::Rights::all(),
                    fs_rights_inheriting: wasi::Rights::all(),
                    fs_flags: wasi::Fdflags::all(),
                },
                JournalEntry::FileDescriptorWriteV1 {
                    fd: 1234,
                    offset: 1234,
                    data: [1u8; 16].to_vec().into(),
                    is_64bit: true,
                },
            ],
            vec![
                JournalEntry::OpenFileDescriptorV1 {
                    fd: 1234,
                    dirfd: 3452345,
                    dirflags: 0,
                    path: "/blah".into(),
                    o_flags: wasi::Oflags::empty(),
                    fs_rights_base: wasi::Rights::all(),
                    fs_rights_inheriting: wasi::Rights::all(),
                    fs_flags: wasi::Fdflags::all(),
                },
                JournalEntry::FileDescriptorWriteV1 {
                    fd: 1234,
                    offset: 1234,
                    data: [1u8; 16].to_vec().into(),
                    is_64bit: true,
                },
            ],
        )
        .unwrap()
    }

    #[tracing_test::traced_test]
    #[test]
    pub fn test_compact_file_system_write_survives_close() {
        run_test(
            vec![
                JournalEntry::OpenFileDescriptorV1 {
                    fd: 1234,
                    dirfd: 3452345,
                    dirflags: 0,
                    path: "/blah".into(),
                    o_flags: wasi::Oflags::empty(),
                    fs_rights_base: wasi::Rights::all(),
                    fs_rights_inheriting: wasi::Rights::all(),
                    fs_flags: wasi::Fdflags::all(),
                },
                JournalEntry::FileDescriptorWriteV1 {
                    fd: 1234,
                    offset: 1234,
                    data: [1u8; 16].to_vec().into(),
                    is_64bit: true,
                },
                JournalEntry::CloseFileDescriptorV1 { fd: 1234 },
            ],
            vec![
                JournalEntry::OpenFileDescriptorV1 {
                    fd: 1234,
                    dirfd: 3452345,
                    dirflags: 0,
                    path: "/blah".into(),
                    o_flags: wasi::Oflags::empty(),
                    fs_rights_base: wasi::Rights::all(),
                    fs_rights_inheriting: wasi::Rights::all(),
                    fs_flags: wasi::Fdflags::all(),
                },
                JournalEntry::FileDescriptorWriteV1 {
                    fd: 1234,
                    offset: 1234,
                    data: [1u8; 16].to_vec().into(),
                    is_64bit: true,
                },
                JournalEntry::CloseFileDescriptorV1 { fd: 1234 },
            ],
        )
        .unwrap()
    }

    #[tracing_test::traced_test]
    #[test]
    pub fn test_compact_file_system_write_survives_exit() {
        run_test(
            vec![
                JournalEntry::OpenFileDescriptorV1 {
                    fd: 1234,
                    dirfd: 3452345,
                    dirflags: 0,
                    path: "/blah".into(),
                    o_flags: wasi::Oflags::empty(),
                    fs_rights_base: wasi::Rights::all(),
                    fs_rights_inheriting: wasi::Rights::all(),
                    fs_flags: wasi::Fdflags::all(),
                },
                JournalEntry::FileDescriptorWriteV1 {
                    fd: 1234,
                    offset: 1234,
                    data: [1u8; 16].to_vec().into(),
                    is_64bit: true,
                },
                JournalEntry::ProcessExitV1 { exit_code: None },
            ],
            vec![
                JournalEntry::OpenFileDescriptorV1 {
                    fd: 1234,
                    dirfd: 3452345,
                    dirflags: 0,
                    path: "/blah".into(),
                    o_flags: wasi::Oflags::empty(),
                    fs_rights_base: wasi::Rights::all(),
                    fs_rights_inheriting: wasi::Rights::all(),
                    fs_flags: wasi::Fdflags::all(),
                },
                JournalEntry::FileDescriptorWriteV1 {
                    fd: 1234,
                    offset: 1234,
                    data: [1u8; 16].to_vec().into(),
                    is_64bit: true,
                },
                JournalEntry::ProcessExitV1 { exit_code: None },
            ],
        )
        .unwrap()
    }

    #[tracing_test::traced_test]
    #[test]
    pub fn test_compact_file_system_read_is_ignored() {
        run_test(
            vec![
                JournalEntry::OpenFileDescriptorV1 {
                    fd: 1234,
                    dirfd: 3452345,
                    dirflags: 0,
                    path: "/blah".into(),
                    o_flags: wasi::Oflags::empty(),
                    fs_rights_base: wasi::Rights::all(),
                    fs_rights_inheriting: wasi::Rights::all(),
                    fs_flags: wasi::Fdflags::all(),
                },
                JournalEntry::FileDescriptorSeekV1 {
                    fd: 1234,
                    offset: 1234,
                    whence: wasi::Whence::End,
                },
                JournalEntry::CloseFileDescriptorV1 { fd: 1234 },
            ],
            Vec::new(),
        )
        .unwrap()
    }

    #[tracing_test::traced_test]
    #[test]
    pub fn test_compact_file_system_touch() {
        run_test(
            vec![
                JournalEntry::OpenFileDescriptorV1 {
                    fd: 1234,
                    dirfd: 3452345,
                    dirflags: 0,
                    path: "/blah".into(),
                    o_flags: wasi::Oflags::CREATE | wasi::Oflags::TRUNC,
                    fs_rights_base: wasi::Rights::all(),
                    fs_rights_inheriting: wasi::Rights::all(),
                    fs_flags: wasi::Fdflags::all(),
                },
                JournalEntry::CloseFileDescriptorV1 { fd: 1234 },
                JournalEntry::ProcessExitV1 { exit_code: None },
            ],
            vec![
                JournalEntry::OpenFileDescriptorV1 {
                    fd: 1234,
                    dirfd: 3452345,
                    dirflags: 0,
                    path: "/blah".into(),
                    o_flags: wasi::Oflags::CREATE | wasi::Oflags::TRUNC,
                    fs_rights_base: wasi::Rights::all(),
                    fs_rights_inheriting: wasi::Rights::all(),
                    fs_flags: wasi::Fdflags::all(),
                },
                JournalEntry::CloseFileDescriptorV1 { fd: 1234 },
                JournalEntry::ProcessExitV1 { exit_code: None },
            ],
        )
        .unwrap()
    }

    #[tracing_test::traced_test]
    #[test]
    pub fn test_compact_file_system_redundant_file() {
        run_test(
            vec![
                JournalEntry::OpenFileDescriptorV1 {
                    fd: 1234,
                    dirfd: 3452345,
                    dirflags: 0,
                    path: "/blah".into(),
                    o_flags: wasi::Oflags::CREATE | wasi::Oflags::TRUNC,
                    fs_rights_base: wasi::Rights::all(),
                    fs_rights_inheriting: wasi::Rights::all(),
                    fs_flags: wasi::Fdflags::all(),
                },
                JournalEntry::FileDescriptorWriteV1 {
                    fd: 1234,
                    offset: 1234,
                    data: [5u8; 16].to_vec().into(),
                    is_64bit: true,
                },
                JournalEntry::CloseFileDescriptorV1 { fd: 1234 },
                JournalEntry::OpenFileDescriptorV1 {
                    fd: 1235,
                    dirfd: 3452345,
                    dirflags: 0,
                    path: "/blah".into(),
                    o_flags: wasi::Oflags::CREATE | wasi::Oflags::TRUNC,
                    fs_rights_base: wasi::Rights::all(),
                    fs_rights_inheriting: wasi::Rights::all(),
                    fs_flags: wasi::Fdflags::all(),
                },
                JournalEntry::FileDescriptorWriteV1 {
                    fd: 1235,
                    offset: 1234,
                    data: [6u8; 16].to_vec().into(),
                    is_64bit: true,
                },
                JournalEntry::CloseFileDescriptorV1 { fd: 1235 },
            ],
            vec![
                JournalEntry::OpenFileDescriptorV1 {
                    fd: 1235,
                    dirfd: 3452345,
                    dirflags: 0,
                    path: "/blah".into(),
                    o_flags: wasi::Oflags::CREATE | wasi::Oflags::TRUNC,
                    fs_rights_base: wasi::Rights::all(),
                    fs_rights_inheriting: wasi::Rights::all(),
                    fs_flags: wasi::Fdflags::all(),
                },
                JournalEntry::FileDescriptorWriteV1 {
                    fd: 1235,
                    offset: 1234,
                    data: [6u8; 16].to_vec().into(),
                    is_64bit: true,
                },
                JournalEntry::CloseFileDescriptorV1 { fd: 1235 },
            ],
        )
        .unwrap()
    }

    #[tracing_test::traced_test]
    #[test]
    pub fn test_compact_file_system_ignore_double_writes() {
        run_test(
            vec![
                JournalEntry::OpenFileDescriptorV1 {
                    fd: 1234,
                    dirfd: 3452345,
                    dirflags: 0,
                    path: "/blah".into(),
                    o_flags: wasi::Oflags::empty(),
                    fs_rights_base: wasi::Rights::all(),
                    fs_rights_inheriting: wasi::Rights::all(),
                    fs_flags: wasi::Fdflags::all(),
                },
                JournalEntry::FileDescriptorWriteV1 {
                    fd: 1234,
                    offset: 1234,
                    data: [1u8; 16].to_vec().into(),
                    is_64bit: true,
                },
                JournalEntry::FileDescriptorWriteV1 {
                    fd: 1234,
                    offset: 1234,
                    data: [5u8; 16].to_vec().into(),
                    is_64bit: true,
                },
                JournalEntry::CloseFileDescriptorV1 { fd: 1234 },
            ],
            vec![
                JournalEntry::OpenFileDescriptorV1 {
                    fd: 1234,
                    dirfd: 3452345,
                    dirflags: 0,
                    path: "/blah".into(),
                    o_flags: wasi::Oflags::empty(),
                    fs_rights_base: wasi::Rights::all(),
                    fs_rights_inheriting: wasi::Rights::all(),
                    fs_flags: wasi::Fdflags::all(),
                },
                JournalEntry::FileDescriptorWriteV1 {
                    fd: 1234,
                    offset: 1234,
                    data: [5u8; 16].to_vec().into(),
                    is_64bit: true,
                },
                JournalEntry::CloseFileDescriptorV1 { fd: 1234 },
            ],
        )
        .unwrap()
    }

    #[tracing_test::traced_test]
    #[test]
    pub fn test_compact_file_system_create_directory() {
        run_test(
            vec![JournalEntry::CreateDirectoryV1 {
                fd: 1234,
                path: "/blah".into(),
            }],
            vec![JournalEntry::CreateDirectoryV1 {
                fd: 1234,
                path: "/blah".into(),
            }],
        )
        .unwrap()
    }

    #[tracing_test::traced_test]
    #[test]
    pub fn test_compact_file_system_redundant_create_directory() {
        run_test(
            vec![
                JournalEntry::CreateDirectoryV1 {
                    fd: 1234,
                    path: "/blah".into(),
                },
                JournalEntry::CreateDirectoryV1 {
                    fd: 1235,
                    path: "/blah".into(),
                },
            ],
            vec![JournalEntry::CreateDirectoryV1 {
                fd: 1234,
                path: "/blah".into(),
            }],
        )
        .unwrap()
    }

    #[tracing_test::traced_test]
    #[test]
    pub fn test_compact_duplicate_tty() {
        run_test(
            vec![
                JournalEntry::TtySetV1 {
                    tty: Tty {
                        cols: 123,
                        rows: 65,
                        width: 2341,
                        height: 573457,
                        stdin_tty: true,
                        stdout_tty: true,
                        stderr_tty: true,
                        echo: true,
                        line_buffered: true,
                    },
                    line_feeds: true,
                },
                JournalEntry::TtySetV1 {
                    tty: Tty {
                        cols: 12,
                        rows: 65,
                        width: 2341,
                        height: 573457,
                        stdin_tty: true,
                        stdout_tty: false,
                        stderr_tty: true,
                        echo: true,
                        line_buffered: true,
                    },
                    line_feeds: true,
                },
            ],
            vec![JournalEntry::TtySetV1 {
                tty: Tty {
                    cols: 12,
                    rows: 65,
                    width: 2341,
                    height: 573457,
                    stdin_tty: true,
                    stdout_tty: false,
                    stderr_tty: true,
                    echo: true,
                    line_buffered: true,
                },
                line_feeds: true,
            }],
        )
        .unwrap()
    }
}
