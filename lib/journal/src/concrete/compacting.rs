use std::{
    collections::{HashMap, HashSet},
    ops::{DerefMut, Range},
    sync::{Arc, Mutex},
};
use wasmer_wasix_types::wasi;

use super::*;

pub type Fd = u32;

/// Subgroup of events that may or may not be retained in the
/// final journal as it is compacted.
///
/// By grouping events into subevents it makes it possible to ignore an
/// entire subgroup of events which are superseeded by a later event. For
/// example, all the events involved in creating a file are irrelevant if
/// that file is later deleted.
#[derive(Debug, Default)]
struct SubGroupOfevents {
    /// List of all the events that will be transferred over
    /// to the compacted journal if this sub group is selected
    /// to be carried over
    events: Vec<usize>,
    /// The path metadata attached to this sub group of events
    /// is used to discard all subgroups related to a particular
    /// path of a file or directory. This is especially important
    /// if that file is later deleted and hence all the events
    /// related to it are no longer relevant
    path: Option<String>,
    /// The write map allows the ccompacted to only keep the
    /// events relevant to the final outcome of a compacted
    /// journal rather than written regions that are later
    /// overridden. This is a crude write map that does not
    /// deal with overlapping writes (they still remain)
    /// However in the majority of cases this will remove
    /// duplicates while retaining a simple implementation
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

/// Index of a group of subevents in the journal which relate to a particular
/// collective impact. For example. Creating a new file which may consist of
/// an event to open a file, the events for writing the file data and the
/// closing of the file are all related to a group of sub events that make
/// up the act of creating that file. During compaction these events
/// will be grouped together so they can be retained or discarded based
/// on the final deterministic outcome of the entire log.
///
/// By grouping events into subevents it makes it possible to ignore an
/// entire subgroup of events which are superseeded by a later event. For
/// example, all the events involved in creating a file are irrelevant if
/// that file is later deleted.
#[derive(Debug, Clone, Copy, PartialOrd, Ord, PartialEq, Eq, Hash)]
struct SubGroupIndex(u64);

#[derive(Debug)]
struct State {
    /// The descriptor seed is used generate descriptor lookups
    descriptor_seed: u64,
    // We maintain a memory map of the events that are significant
    memory_map: HashMap<MemoryRange, usize>,
    // List of all the snapshots
    snapshots: Vec<usize>,
    // Last tty event thats been set
    tty: Option<usize>,
    // The last change directory event
    chdir: Option<usize>,
    // Last exit that signals the exiting of the process
    process_exit: Option<usize>,
    // Last event that initialized the module
    init_module: Option<usize>,
    // Events that create a particular directory
    create_directory: HashMap<String, SubGroupIndex>,
    // Events that remove a particular directory
    remove_directory: HashMap<String, usize>,
    // Events that unlink a file
    unlink_file: HashMap<String, usize>,
    // Thread events are only maintained while the thread and the
    // process are still running
    thread_map: HashMap<u32, usize>,
    // Thread events are only maintained while the thread and the
    // process are still running
    staged_thread_map: HashMap<u32, usize>,
    // Sockets that are open and not yet closed are kept here
    open_sockets: HashMap<Fd, SubGroupIndex>,
    // Sockets that are open and not yet closed are kept here
    accepted_sockets: HashMap<Fd, SubGroupIndex>,
    // Open pipes have two file descriptors that are associated with
    // them. We keep track of both of them
    open_pipes: HashMap<Fd, SubGroupIndex>,
    // Any descriptors are assumed to be read only operations until
    // they actually do something that changes the system
    suspect_descriptors: HashMap<Fd, SubGroupIndex>,
    // Any descriptors are assumed to be read only operations until
    // they actually do something that changes the system
    keep_descriptors: HashMap<Fd, SubGroupIndex>,
    kept_descriptors: Vec<SubGroupIndex>,
    // We put the IO related to stdio into a special list
    // which can be purged when the program exits as its no longer
    // important.
    stdio_descriptors: HashMap<Fd, SubGroupIndex>,
    // Event objects handle events from other parts of the process
    // and feed them to a processing thread
    event_descriptors: HashMap<Fd, SubGroupIndex>,
    // Epoll events
    epoll_descriptors: HashMap<Fd, SubGroupIndex>,
    // We abstract the descriptor state so that multiple file descriptors
    // can refer to the same file descriptors
    sub_events: HashMap<SubGroupIndex, SubGroupOfevents>,
    // Everything that will be retained during the next compact
    whitelist: HashSet<usize>,
    // We use an event index to track what to keep
    event_index: usize,
    // The delta list is used for all the events that happened
    // after a compact started
    delta_list: Option<Vec<usize>>,
    // The inner journal that we will write to
    inner_tx: Box<DynWritableJournal>,
    // The inner journal that we read from
    inner_rx: Box<DynReadableJournal>,
}

impl State {
    fn create_filter<J>(
        &self,
        inner: J,
    ) -> FilteredJournal<Box<DynWritableJournal>, Box<DynReadableJournal>>
    where
        J: Journal,
    {
        let (w, r) = inner.split();
        self.create_split_filter(w, r)
    }

    fn create_split_filter<W, R>(&self, writer: W, reader: R) -> FilteredJournal<W, R>
    where
        W: WritableJournal,
        R: ReadableJournal,
    {
        let mut filter = FilteredJournalBuilder::new()
            .with_filter_events(self.whitelist.clone().into_iter().collect());

        for event_index in self
            .tty
            .as_ref()
            .into_iter()
            .chain(self.chdir.as_ref().into_iter())
            .chain(self.process_exit.as_ref().into_iter())
            .chain(self.init_module.as_ref().into_iter())
            .chain(self.snapshots.iter())
            .chain(self.memory_map.values())
            .chain(self.thread_map.values())
            .chain(self.remove_directory.values())
            .chain(self.unlink_file.values())
            .cloned()
        {
            filter.add_event_to_whitelist(event_index);
        }
        for d in self
            .create_directory
            .values()
            .filter_map(|l| self.sub_events.get(l))
            .chain(
                self.suspect_descriptors
                    .values()
                    .filter_map(|l| self.sub_events.get(l)),
            )
            .chain(
                self.keep_descriptors
                    .values()
                    .filter_map(|l| self.sub_events.get(l)),
            )
            .chain(
                self.kept_descriptors
                    .iter()
                    .filter_map(|l| self.sub_events.get(l)),
            )
            .chain(
                self.open_sockets
                    .values()
                    .filter_map(|l| self.sub_events.get(l)),
            )
            .chain(
                self.accepted_sockets
                    .values()
                    .filter_map(|l| self.sub_events.get(l)),
            )
            .chain(
                self.event_descriptors
                    .values()
                    .filter_map(|l| self.sub_events.get(l)),
            )
            .chain(
                self.epoll_descriptors
                    .values()
                    .filter_map(|l| self.sub_events.get(l)),
            )
            .chain(
                self.open_pipes
                    .values()
                    .filter_map(|l| self.sub_events.get(l)),
            )
            .chain(
                self.stdio_descriptors
                    .values()
                    .filter_map(|l| self.sub_events.get(l)),
            )
        {
            for e in d.events.iter() {
                filter.add_event_to_whitelist(*e);
            }
            for e in d.write_map.values() {
                filter.add_event_to_whitelist(*e);
            }
        }
        filter.build_split(writer, reader)
    }

    fn insert_new_sub_events(&mut self, event_index: usize) -> SubGroupIndex {
        let lookup = SubGroupIndex(self.descriptor_seed);
        self.descriptor_seed += 1;

        self.sub_events
            .entry(lookup)
            .or_default()
            .events
            .push(event_index);

        lookup
    }

    fn append_to_sub_events(&mut self, lookup: &SubGroupIndex, event_index: usize) {
        if let Some(state) = self.sub_events.get_mut(lookup) {
            state.events.push(event_index);
        }
    }

    fn set_path_for_sub_events(&mut self, lookup: &SubGroupIndex, path: &str) {
        if let Some(state) = self.sub_events.get_mut(lookup) {
            state.path = Some(path.to_string());
        }
    }

    fn cancel_sub_events_by_path(&mut self, path: &str) {
        let test = Some(path.to_string());
        self.sub_events.retain(|_, d| d.path != test);
    }

    fn solidify_sub_events_by_path(&mut self, path: &str) {
        let test = Some(path.to_string());
        self.sub_events
            .iter_mut()
            .filter(|(_, d)| d.path == test)
            .for_each(|(_, d)| {
                d.path.take();
            })
    }

    fn find_sub_events(&self, fd: &u32) -> Option<SubGroupIndex> {
        self.suspect_descriptors
            .get(fd)
            .cloned()
            .or_else(|| self.open_sockets.get(fd).cloned())
            .or_else(|| self.accepted_sockets.get(fd).cloned())
            .or_else(|| self.open_pipes.get(fd).cloned())
            .or_else(|| self.keep_descriptors.get(fd).cloned())
            .or_else(|| self.event_descriptors.get(fd).cloned())
            .or_else(|| self.stdio_descriptors.get(fd).cloned())
    }

    fn find_sub_events_and_append(&mut self, fd: &u32, event_index: usize) {
        if let Some(lookup) = self.find_sub_events(fd) {
            self.append_to_sub_events(&lookup, event_index);
        }
    }

    fn clear_run_sub_events(&mut self) {
        self.accepted_sockets.clear();
        self.event_descriptors.clear();
        self.memory_map.clear();
        self.open_pipes.clear();
        self.open_sockets.clear();
        self.snapshots.clear();
        self.staged_thread_map.clear();
        self.stdio_descriptors.clear();
        self.suspect_descriptors.clear();
        self.thread_map.clear();
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

#[derive(Debug)]
pub struct CompactingJournalRx {
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
            chdir: None,
            process_exit: None,
            init_module: None,
            snapshots: Default::default(),
            memory_map: Default::default(),
            thread_map: Default::default(),
            staged_thread_map: Default::default(),
            open_sockets: Default::default(),
            accepted_sockets: Default::default(),
            open_pipes: Default::default(),
            create_directory: Default::default(),
            remove_directory: Default::default(),
            unlink_file: Default::default(),
            suspect_descriptors: Default::default(),
            keep_descriptors: Default::default(),
            kept_descriptors: Default::default(),
            stdio_descriptors: Default::default(),
            event_descriptors: Default::default(),
            epoll_descriptors: Default::default(),
            descriptor_seed: 0,
            sub_events: Default::default(),
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

    /// Creates a filter jounral which will write all
    /// its events to an inner journal
    pub fn create_filter<J>(
        &self,
        inner: J,
    ) -> FilteredJournal<Box<DynWritableJournal>, Box<DynReadableJournal>>
    where
        J: Journal,
    {
        self.tx.create_filter(inner)
    }

    /// Creates a filter journal which will write all
    /// its events to writer and readers supplied
    pub fn create_split_filter<W, R>(&self, writer: W, reader: R) -> FilteredJournal<W, R>
    where
        W: WritableJournal,
        R: ReadableJournal,
    {
        self.tx.create_split_filter(writer, reader)
    }
}

/// Represents the results of a compaction operation
#[derive(Debug, Default)]
pub struct CompactResult {
    pub total_size: u64,
    pub total_events: usize,
}

impl CompactingJournalTx {
    pub fn create_filter<J>(
        &self,
        inner: J,
    ) -> FilteredJournal<Box<DynWritableJournal>, Box<DynReadableJournal>>
    where
        J: Journal,
    {
        let state = self.state.lock().unwrap();
        state.create_filter(inner)
    }

    pub fn create_split_filter<W, R>(&self, writer: W, reader: R) -> FilteredJournal<W, R>
    where
        W: WritableJournal,
        R: ReadableJournal,
    {
        let state = self.state.lock().unwrap();
        state.create_split_filter(writer, reader)
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
    #[allow(clippy::assigning_clones)]
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
                state.staged_thread_map.insert(*id, event_index);
            }
            JournalEntry::CloseThreadV1 { id, .. } => {
                state.staged_thread_map.remove(id);
            }
            JournalEntry::SnapshotV1 { .. } => {
                state.thread_map = state.staged_thread_map.clone();
                state.snapshots.push(event_index);
            }
            JournalEntry::ProcessExitV1 { .. } => {
                state.clear_run_sub_events();
                state.process_exit = Some(event_index);
            }
            JournalEntry::TtySetV1 { .. } => {
                state.tty.replace(event_index);
            }
            JournalEntry::ChangeDirectoryV1 { .. } => {
                state.chdir.replace(event_index);
            }
            JournalEntry::CreateEventV1 { fd, .. } => {
                let lookup = state.insert_new_sub_events(event_index);
                state.event_descriptors.insert(*fd, lookup);
            }
            JournalEntry::OpenFileDescriptorV1 {
                fd, o_flags, path, ..
            }
            | JournalEntry::OpenFileDescriptorV2 {
                fd, o_flags, path, ..
            } => {
                // Creating a file and erasing anything that was there before means
                // the entire create branch that exists before this one can be ignored
                let path = path.to_string();
                if o_flags.contains(wasi::Oflags::CREATE) && o_flags.contains(wasi::Oflags::TRUNC) {
                    state.cancel_sub_events_by_path(path.as_ref());
                }
                // All file descriptors are opened in a suspect state which
                // means if they are closed without modifying the file system
                // then the events will be ignored.
                let lookup = state.insert_new_sub_events(event_index);
                state.set_path_for_sub_events(&lookup, path.as_ref());

                // There is an exception to the rule which is if the create
                // flag is specified its always recorded as a mutating operation
                // because it may create a file that does not exist on the file system
                if o_flags.contains(wasi::Oflags::CREATE) {
                    state.keep_descriptors.insert(*fd, lookup);
                } else {
                    state.suspect_descriptors.insert(*fd, lookup);
                }
            }
            // Things that modify a file descriptor mean that it is
            // no longer suspect and thus it needs to be kept
            JournalEntry::FileDescriptorAdviseV1 { fd, .. }
            | JournalEntry::FileDescriptorAllocateV1 { fd, .. }
            | JournalEntry::FileDescriptorSetTimesV1 { fd, .. }
            | JournalEntry::FileDescriptorWriteV1 { fd, .. }
            | JournalEntry::FileDescriptorSetRightsV1 { fd, .. }
            | JournalEntry::FileDescriptorSetSizeV1 { fd, .. } => {
                // Its no longer suspect
                if let Some(lookup) = state.suspect_descriptors.remove(fd) {
                    state.keep_descriptors.insert(*fd, lookup);
                }

                // If its stdio then we need to create the descriptor if its not there already
                if *fd <= 3 && !state.stdio_descriptors.contains_key(fd) {
                    let lookup = state.insert_new_sub_events(event_index);
                    state.stdio_descriptors.insert(*fd, lookup);
                }

                // Update the state
                if let Some(state) = state
                    .find_sub_events(fd)
                    .and_then(|lookup| state.sub_events.get_mut(&lookup))
                {
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
                }
            }
            // Seeks to a particular position within
            JournalEntry::FileDescriptorSeekV1 { fd, .. }
            | JournalEntry::FileDescriptorSetFdFlagsV1 { fd, .. }
            | JournalEntry::FileDescriptorSetFlagsV1 { fd, .. } => {
                // If its stdio then we need to create the descriptor if its not there already
                if *fd <= 3 && !state.stdio_descriptors.contains_key(fd) {
                    let lookup = state.insert_new_sub_events(event_index);
                    state.stdio_descriptors.insert(*fd, lookup);
                }
                state.find_sub_events_and_append(fd, event_index);
            }
            // We keep non-mutable events for file descriptors that are suspect
            JournalEntry::SocketBindV1 { fd, .. }
            | JournalEntry::SocketSendFileV1 { socket_fd: fd, .. }
            | JournalEntry::SocketSendToV1 { fd, .. }
            | JournalEntry::SocketSendV1 { fd, .. }
            | JournalEntry::SocketSetOptFlagV1 { fd, .. }
            | JournalEntry::SocketSetOptSizeV1 { fd, .. }
            | JournalEntry::SocketSetOptTimeV1 { fd, .. }
            | JournalEntry::SocketShutdownV1 { fd, .. }
            | JournalEntry::SocketListenV1 { fd, .. }
            | JournalEntry::SocketJoinIpv4MulticastV1 { fd, .. }
            | JournalEntry::SocketJoinIpv6MulticastV1 { fd, .. }
            | JournalEntry::SocketLeaveIpv4MulticastV1 { fd, .. }
            | JournalEntry::SocketLeaveIpv6MulticastV1 { fd, .. } => {
                state.find_sub_events_and_append(fd, event_index);
            }
            // Closing a file can stop all the events from appearing in the
            // journal at all
            JournalEntry::CloseFileDescriptorV1 { fd } => {
                if let Some(lookup) = state.open_sockets.remove(fd) {
                    state.sub_events.remove(&lookup);
                } else if let Some(lookup) = state.accepted_sockets.remove(fd) {
                    state.sub_events.remove(&lookup);
                } else if let Some(lookup) = state.open_pipes.remove(fd) {
                    state.sub_events.remove(&lookup);
                } else if let Some(lookup) = state.suspect_descriptors.remove(fd) {
                    state.sub_events.remove(&lookup);
                } else if let Some(lookup) = state.event_descriptors.remove(fd) {
                    state.sub_events.remove(&lookup);
                } else if let Some(lookup) = state.epoll_descriptors.remove(fd) {
                    state.sub_events.remove(&lookup);
                } else if let Some(lookup) = state.keep_descriptors.remove(fd) {
                    state.append_to_sub_events(&lookup, event_index);
                    state.kept_descriptors.push(lookup);
                } else {
                    state.find_sub_events_and_append(fd, event_index);
                }
            }
            // Duplicating the file descriptor
            JournalEntry::DuplicateFileDescriptorV1 {
                original_fd,
                copied_fd,
            }
            | JournalEntry::DuplicateFileDescriptorV2 {
                original_fd,
                copied_fd,
                ..
            } => {
                if let Some(lookup) = state.suspect_descriptors.get(original_fd).cloned() {
                    state.suspect_descriptors.insert(*copied_fd, lookup);
                } else if let Some(lookup) = state.keep_descriptors.get(original_fd).cloned() {
                    state.keep_descriptors.insert(*copied_fd, lookup);
                } else if let Some(lookup) = state.stdio_descriptors.get(original_fd).cloned() {
                    state.stdio_descriptors.insert(*copied_fd, lookup);
                } else if let Some(lookup) = state.open_pipes.get(original_fd).cloned() {
                    state.open_pipes.insert(*copied_fd, lookup);
                } else if let Some(lookup) = state.open_sockets.get(original_fd).cloned() {
                    state.open_sockets.insert(*copied_fd, lookup);
                } else if let Some(lookup) = state.accepted_sockets.get(original_fd).cloned() {
                    state.accepted_sockets.insert(*copied_fd, lookup);
                } else if let Some(lookup) = state.event_descriptors.get(original_fd).cloned() {
                    state.event_descriptors.insert(*copied_fd, lookup);
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
                } else if let Some(lookup) = state.open_pipes.remove(old_fd) {
                    state.open_pipes.insert(*new_fd, lookup);
                } else if let Some(lookup) = state.open_sockets.remove(old_fd) {
                    state.open_sockets.insert(*new_fd, lookup);
                } else if let Some(lookup) = state.open_sockets.remove(old_fd) {
                    state.accepted_sockets.insert(*new_fd, lookup);
                } else if let Some(lookup) = state.event_descriptors.remove(old_fd) {
                    state.event_descriptors.insert(*new_fd, lookup);
                }
            }
            // Creating a new directory only needs to be done once
            JournalEntry::CreateDirectoryV1 { path, .. } => {
                let path = path.to_string();

                // Newly created directories are stored as a set of .
                #[allow(clippy::map_entry)]
                if !state.create_directory.contains_key(&path) {
                    let lookup = state.insert_new_sub_events(event_index);
                    state.set_path_for_sub_events(&lookup, &path);
                    state.create_directory.insert(path, lookup);
                };
            }
            // Deleting a directory only needs to be done once
            JournalEntry::RemoveDirectoryV1 { path, .. } => {
                let path = path.to_string();
                state.create_directory.remove(&path);
                state.remove_directory.insert(path, event_index);
            }
            // Unlinks the file from the file system
            JournalEntry::UnlinkFileV1 { path, .. } => {
                state.cancel_sub_events_by_path(path.as_ref());
                state.unlink_file.insert(path.to_string(), event_index);
            }
            // Renames may update some of the tracking functions
            JournalEntry::PathRenameV1 {
                old_path, new_path, ..
            } => {
                state.solidify_sub_events_by_path(old_path.as_ref());
                state.cancel_sub_events_by_path(new_path.as_ref());
                state.whitelist.insert(event_index);
            }
            // Update all the directory operations
            JournalEntry::PathSetTimesV1 { path, .. } => {
                let path = path.to_string();
                if let Some(lookup) = state.create_directory.get(&path).cloned() {
                    state.append_to_sub_events(&lookup, event_index);
                } else if !state.remove_directory.contains_key(&path) {
                    state.whitelist.insert(event_index);
                }
            }
            // Pipes that remain open at the end will be added
            JournalEntry::CreatePipeV1 { read_fd, write_fd } => {
                let lookup = state.insert_new_sub_events(event_index);
                state.open_pipes.insert(*read_fd, lookup);
                state.open_pipes.insert(*write_fd, lookup);
            }
            // Epoll events
            JournalEntry::EpollCreateV1 { fd } => {
                let lookup = state.insert_new_sub_events(event_index);
                state.epoll_descriptors.insert(*fd, lookup);
            }
            JournalEntry::EpollCtlV1 { epfd, fd, .. } => {
                if state.find_sub_events(fd).is_some() {
                    state.find_sub_events_and_append(epfd, event_index);
                }
            }
            JournalEntry::SocketConnectedV1 { fd, .. } => {
                let lookup = state.insert_new_sub_events(event_index);
                state.accepted_sockets.insert(*fd, lookup);
            }
            // Sockets that are accepted are suspect
            JournalEntry::SocketAcceptedV1 { fd, .. } | JournalEntry::SocketOpenV1 { fd, .. } => {
                let lookup = state.insert_new_sub_events(event_index);
                state.open_sockets.insert(*fd, lookup);
            }
            JournalEntry::SocketPairV1 { fd1, fd2 } => {
                let lookup = state.insert_new_sub_events(event_index);
                state.open_sockets.insert(*fd1, lookup);
                state.open_sockets.insert(*fd2, lookup);
            }
            JournalEntry::InitModuleV1 { .. } => {
                state.clear_run_sub_events();
                state.init_module = Some(event_index);
            }
            JournalEntry::ClearEtherealV1 => {
                state.clear_run_sub_events();
            }
            JournalEntry::SetClockTimeV1 { .. }
            | JournalEntry::PortAddAddrV1 { .. }
            | JournalEntry::PortDelAddrV1 { .. }
            | JournalEntry::PortAddrClearV1
            | JournalEntry::PortBridgeV1 { .. }
            | JournalEntry::PortUnbridgeV1
            | JournalEntry::PortDhcpAcquireV1
            | JournalEntry::PortGatewaySetV1 { .. }
            | JournalEntry::PortRouteAddV1 { .. }
            | JournalEntry::PortRouteClearV1
            | JournalEntry::PortRouteDelV1 { .. }
            | JournalEntry::CreateSymbolicLinkV1 { .. }
            | JournalEntry::CreateHardLinkV1 { .. } => {
                state.whitelist.insert(event_index);
            }
        }
        state.inner_tx.write(entry)
    }

    fn flush(&self) -> anyhow::Result<()> {
        self.state.lock().unwrap().inner_tx.flush()
    }

    fn commit(&self) -> anyhow::Result<usize> {
        self.state.lock().unwrap().inner_tx.commit()
    }

    fn rollback(&self) -> anyhow::Result<usize> {
        self.state.lock().unwrap().inner_tx.rollback()
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

    fn commit(&self) -> anyhow::Result<usize> {
        self.tx.commit()
    }

    fn rollback(&self) -> anyhow::Result<usize> {
        self.tx.rollback()
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

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::*;

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
            let record2 = new_records.read()?.map(|r| r.record);
            assert_eq!(Some(record1), record2);
        }
        assert_eq!(
            None,
            new_records.read()?.map(|x| x.record),
            "found unexpected extra records in the compacted journal"
        );

        Ok(())
    }

    // #[tracing_test::traced_test]
    // #[test]
    // pub fn test_compact_purge_duplicate_memory_writes() {
    //     run_test(
    //         vec![
    //             JournalEntry::UpdateMemoryRegionV1 {
    //                 region: 0..16,
    //                 data: [11u8; 16].to_vec().into(),
    //             },
    //             JournalEntry::UpdateMemoryRegionV1 {
    //                 region: 0..16,
    //                 data: [22u8; 16].to_vec().into(),
    //             },
    //         ],
    //         vec![JournalEntry::UpdateMemoryRegionV1 {
    //             region: 0..16,
    //             data: [22u8; 16].to_vec().into(),
    //         }],
    //     )
    //     .unwrap()
    // }
    //
    // #[tracing_test::traced_test]
    // #[test]
    // pub fn test_compact_keep_overlapping_memory() {
    //     run_test(
    //         vec![
    //             JournalEntry::UpdateMemoryRegionV1 {
    //                 region: 0..16,
    //                 data: [11u8; 16].to_vec().into(),
    //             },
    //             JournalEntry::UpdateMemoryRegionV1 {
    //                 region: 20..36,
    //                 data: [22u8; 16].to_vec().into(),
    //             },
    //         ],
    //         vec![
    //             JournalEntry::UpdateMemoryRegionV1 {
    //                 region: 0..16,
    //                 data: [11u8; 16].to_vec().into(),
    //             },
    //             JournalEntry::UpdateMemoryRegionV1 {
    //                 region: 20..36,
    //                 data: [22u8; 16].to_vec().into(),
    //             },
    //         ],
    //     )
    //     .unwrap()
    // }
    //
    // #[tracing_test::traced_test]
    // #[test]
    // pub fn test_compact_keep_adjacent_memory_writes() {
    //     run_test(
    //         vec![
    //             JournalEntry::UpdateMemoryRegionV1 {
    //                 region: 0..16,
    //                 data: [11u8; 16].to_vec().into(),
    //             },
    //             JournalEntry::UpdateMemoryRegionV1 {
    //                 region: 16..32,
    //                 data: [22u8; 16].to_vec().into(),
    //             },
    //         ],
    //         vec![
    //             JournalEntry::UpdateMemoryRegionV1 {
    //                 region: 0..16,
    //                 data: [11u8; 16].to_vec().into(),
    //             },
    //             JournalEntry::UpdateMemoryRegionV1 {
    //                 region: 16..32,
    //                 data: [22u8; 16].to_vec().into(),
    //             },
    //         ],
    //     )
    //     .unwrap()
    // }
    //
    // #[tracing_test::traced_test]
    // #[test]
    // pub fn test_compact_purge_identical_memory_writes() {
    //     run_test(
    //         vec![
    //             JournalEntry::UpdateMemoryRegionV1 {
    //                 region: 0..16,
    //                 data: [11u8; 16].to_vec().into(),
    //             },
    //             JournalEntry::UpdateMemoryRegionV1 {
    //                 region: 0..16,
    //                 data: [11u8; 16].to_vec().into(),
    //             },
    //         ],
    //         vec![JournalEntry::UpdateMemoryRegionV1 {
    //             region: 0..16,
    //             data: [11u8; 16].to_vec().into(),
    //         }],
    //     )
    //     .unwrap()
    // }
    //
    // #[tracing_test::traced_test]
    // #[test]
    // pub fn test_compact_thread_stacks() {
    //     run_test(
    //         vec![
    //             JournalEntry::SetThreadV1 {
    //                 id: 4321.into(),
    //                 call_stack: [44u8; 87].to_vec().into(),
    //                 memory_stack: [55u8; 34].to_vec().into(),
    //                 store_data: [66u8; 70].to_vec().into(),
    //                 is_64bit: true,
    //             },
    //             JournalEntry::SetThreadV1 {
    //                 id: 1234.into(),
    //                 call_stack: [11u8; 124].to_vec().into(),
    //                 memory_stack: [22u8; 51].to_vec().into(),
    //                 store_data: [33u8; 87].to_vec().into(),
    //                 is_64bit: true,
    //             },
    //             JournalEntry::SetThreadV1 {
    //                 id: 65.into(),
    //                 call_stack: [77u8; 34].to_vec().into(),
    //                 memory_stack: [88u8; 51].to_vec().into(),
    //                 store_data: [99u8; 12].to_vec().into(),
    //                 is_64bit: true,
    //             },
    //             JournalEntry::CloseThreadV1 {
    //                 id: 1234.into(),
    //                 exit_code: None,
    //             },
    //         ],
    //         vec![
    //             JournalEntry::SetThreadV1 {
    //                 id: 4321.into(),
    //                 call_stack: [44u8; 87].to_vec().into(),
    //                 memory_stack: [55u8; 34].to_vec().into(),
    //                 store_data: [66u8; 70].to_vec().into(),
    //                 is_64bit: true,
    //             },
    //             JournalEntry::SetThreadV1 {
    //                 id: 65.into(),
    //                 call_stack: [77u8; 34].to_vec().into(),
    //                 memory_stack: [88u8; 51].to_vec().into(),
    //                 store_data: [99u8; 12].to_vec().into(),
    //                 is_64bit: true,
    //             },
    //         ],
    //     )
    //     .unwrap()
    // }
    //
    // #[tracing_test::traced_test]
    // #[test]
    // pub fn test_compact_processed_exited() {
    //     run_test(
    //         vec![
    //             JournalEntry::UpdateMemoryRegionV1 {
    //                 region: 0..16,
    //                 data: [11u8; 16].to_vec().into(),
    //             },
    //             JournalEntry::SetThreadV1 {
    //                 id: 4321.into(),
    //                 call_stack: [44u8; 87].to_vec().into(),
    //                 memory_stack: [55u8; 34].to_vec().into(),
    //                 store_data: [66u8; 70].to_vec().into(),
    //                 is_64bit: true,
    //             },
    //             JournalEntry::SnapshotV1 {
    //                 when: SystemTime::now(),
    //                 trigger: SnapshotTrigger::FirstListen,
    //             },
    //             JournalEntry::OpenFileDescriptorV1 {
    //                 fd: 1234,
    //                 dirfd: 3452345,
    //                 dirflags: 0,
    //                 path: "/blah".into(),
    //                 o_flags: wasi::Oflags::empty(),
    //                 fs_rights_base: wasi::Rights::all(),
    //                 fs_rights_inheriting: wasi::Rights::all(),
    //                 fs_flags: wasi::Fdflags::all(),
    //             },
    //             JournalEntry::ProcessExitV1 { exit_code: None },
    //         ],
    //         vec![JournalEntry::ProcessExitV1 { exit_code: None }],
    //     )
    //     .unwrap()
    // }
    //
    // #[tracing_test::traced_test]
    // #[test]
    // pub fn test_compact_file_system_partial_write_survives() {
    //     run_test(
    //         vec![
    //             JournalEntry::OpenFileDescriptorV1 {
    //                 fd: 1234,
    //                 dirfd: 3452345,
    //                 dirflags: 0,
    //                 path: "/blah".into(),
    //                 o_flags: wasi::Oflags::empty(),
    //                 fs_rights_base: wasi::Rights::all(),
    //                 fs_rights_inheriting: wasi::Rights::all(),
    //                 fs_flags: wasi::Fdflags::all(),
    //             },
    //             JournalEntry::FileDescriptorWriteV1 {
    //                 fd: 1234,
    //                 offset: 1234,
    //                 data: [1u8; 16].to_vec().into(),
    //                 is_64bit: true,
    //             },
    //         ],
    //         vec![
    //             JournalEntry::OpenFileDescriptorV1 {
    //                 fd: 1234,
    //                 dirfd: 3452345,
    //                 dirflags: 0,
    //                 path: "/blah".into(),
    //                 o_flags: wasi::Oflags::empty(),
    //                 fs_rights_base: wasi::Rights::all(),
    //                 fs_rights_inheriting: wasi::Rights::all(),
    //                 fs_flags: wasi::Fdflags::all(),
    //             },
    //             JournalEntry::FileDescriptorWriteV1 {
    //                 fd: 1234,
    //                 offset: 1234,
    //                 data: [1u8; 16].to_vec().into(),
    //                 is_64bit: true,
    //             },
    //         ],
    //     )
    //     .unwrap()
    // }
    //
    // #[tracing_test::traced_test]
    // #[test]
    // pub fn test_compact_file_system_write_survives_close() {
    //     run_test(
    //         vec![
    //             JournalEntry::OpenFileDescriptorV1 {
    //                 fd: 1234,
    //                 dirfd: 3452345,
    //                 dirflags: 0,
    //                 path: "/blah".into(),
    //                 o_flags: wasi::Oflags::empty(),
    //                 fs_rights_base: wasi::Rights::all(),
    //                 fs_rights_inheriting: wasi::Rights::all(),
    //                 fs_flags: wasi::Fdflags::all(),
    //             },
    //             JournalEntry::FileDescriptorWriteV1 {
    //                 fd: 1234,
    //                 offset: 1234,
    //                 data: [1u8; 16].to_vec().into(),
    //                 is_64bit: true,
    //             },
    //             JournalEntry::CloseFileDescriptorV1 { fd: 1234 },
    //         ],
    //         vec![
    //             JournalEntry::OpenFileDescriptorV1 {
    //                 fd: 1234,
    //                 dirfd: 3452345,
    //                 dirflags: 0,
    //                 path: "/blah".into(),
    //                 o_flags: wasi::Oflags::empty(),
    //                 fs_rights_base: wasi::Rights::all(),
    //                 fs_rights_inheriting: wasi::Rights::all(),
    //                 fs_flags: wasi::Fdflags::all(),
    //             },
    //             JournalEntry::FileDescriptorWriteV1 {
    //                 fd: 1234,
    //                 offset: 1234,
    //                 data: [1u8; 16].to_vec().into(),
    //                 is_64bit: true,
    //             },
    //             JournalEntry::CloseFileDescriptorV1 { fd: 1234 },
    //         ],
    //     )
    //     .unwrap()
    // }
    //
    // #[tracing_test::traced_test]
    // #[test]
    // pub fn test_compact_file_system_write_survives_exit() {
    //     run_test(
    //         vec![
    //             JournalEntry::OpenFileDescriptorV1 {
    //                 fd: 1234,
    //                 dirfd: 3452345,
    //                 dirflags: 0,
    //                 path: "/blah".into(),
    //                 o_flags: wasi::Oflags::empty(),
    //                 fs_rights_base: wasi::Rights::all(),
    //                 fs_rights_inheriting: wasi::Rights::all(),
    //                 fs_flags: wasi::Fdflags::all(),
    //             },
    //             JournalEntry::FileDescriptorWriteV1 {
    //                 fd: 1234,
    //                 offset: 1234,
    //                 data: [1u8; 16].to_vec().into(),
    //                 is_64bit: true,
    //             },
    //             JournalEntry::ProcessExitV1 { exit_code: None },
    //         ],
    //         vec![
    //             JournalEntry::OpenFileDescriptorV1 {
    //                 fd: 1234,
    //                 dirfd: 3452345,
    //                 dirflags: 0,
    //                 path: "/blah".into(),
    //                 o_flags: wasi::Oflags::empty(),
    //                 fs_rights_base: wasi::Rights::all(),
    //                 fs_rights_inheriting: wasi::Rights::all(),
    //                 fs_flags: wasi::Fdflags::all(),
    //             },
    //             JournalEntry::FileDescriptorWriteV1 {
    //                 fd: 1234,
    //                 offset: 1234,
    //                 data: [1u8; 16].to_vec().into(),
    //                 is_64bit: true,
    //             },
    //             JournalEntry::ProcessExitV1 { exit_code: None },
    //         ],
    //     )
    //     .unwrap()
    // }
    //
    // #[tracing_test::traced_test]
    // #[test]
    // pub fn test_compact_file_system_read_is_ignored() {
    //     run_test(
    //         vec![
    //             JournalEntry::OpenFileDescriptorV1 {
    //                 fd: 1234,
    //                 dirfd: 3452345,
    //                 dirflags: 0,
    //                 path: "/blah".into(),
    //                 o_flags: wasi::Oflags::empty(),
    //                 fs_rights_base: wasi::Rights::all(),
    //                 fs_rights_inheriting: wasi::Rights::all(),
    //                 fs_flags: wasi::Fdflags::all(),
    //             },
    //             JournalEntry::FileDescriptorSeekV1 {
    //                 fd: 1234,
    //                 offset: 1234,
    //                 whence: wasi::Whence::End,
    //             },
    //             JournalEntry::CloseFileDescriptorV1 { fd: 1234 },
    //         ],
    //         Vec::new(),
    //     )
    //     .unwrap()
    // }
    //
    // #[tracing_test::traced_test]
    // #[test]
    // pub fn test_compact_file_system_touch() {
    //     run_test(
    //         vec![
    //             JournalEntry::OpenFileDescriptorV1 {
    //                 fd: 1234,
    //                 dirfd: 3452345,
    //                 dirflags: 0,
    //                 path: "/blah".into(),
    //                 o_flags: wasi::Oflags::CREATE | wasi::Oflags::TRUNC,
    //                 fs_rights_base: wasi::Rights::all(),
    //                 fs_rights_inheriting: wasi::Rights::all(),
    //                 fs_flags: wasi::Fdflags::all(),
    //             },
    //             JournalEntry::CloseFileDescriptorV1 { fd: 1234 },
    //             JournalEntry::ProcessExitV1 { exit_code: None },
    //         ],
    //         vec![
    //             JournalEntry::OpenFileDescriptorV1 {
    //                 fd: 1234,
    //                 dirfd: 3452345,
    //                 dirflags: 0,
    //                 path: "/blah".into(),
    //                 o_flags: wasi::Oflags::CREATE | wasi::Oflags::TRUNC,
    //                 fs_rights_base: wasi::Rights::all(),
    //                 fs_rights_inheriting: wasi::Rights::all(),
    //                 fs_flags: wasi::Fdflags::all(),
    //             },
    //             JournalEntry::CloseFileDescriptorV1 { fd: 1234 },
    //             JournalEntry::ProcessExitV1 { exit_code: None },
    //         ],
    //     )
    //     .unwrap()
    // }
    //
    // #[tracing_test::traced_test]
    // #[test]
    // pub fn test_compact_file_system_redundant_file() {
    //     run_test(
    //         vec![
    //             JournalEntry::OpenFileDescriptorV1 {
    //                 fd: 1234,
    //                 dirfd: 3452345,
    //                 dirflags: 0,
    //                 path: "/blah".into(),
    //                 o_flags: wasi::Oflags::CREATE | wasi::Oflags::TRUNC,
    //                 fs_rights_base: wasi::Rights::all(),
    //                 fs_rights_inheriting: wasi::Rights::all(),
    //                 fs_flags: wasi::Fdflags::all(),
    //             },
    //             JournalEntry::FileDescriptorWriteV1 {
    //                 fd: 1234,
    //                 offset: 1234,
    //                 data: [5u8; 16].to_vec().into(),
    //                 is_64bit: true,
    //             },
    //             JournalEntry::CloseFileDescriptorV1 { fd: 1234 },
    //             JournalEntry::OpenFileDescriptorV1 {
    //                 fd: 1235,
    //                 dirfd: 3452345,
    //                 dirflags: 0,
    //                 path: "/blah".into(),
    //                 o_flags: wasi::Oflags::CREATE | wasi::Oflags::TRUNC,
    //                 fs_rights_base: wasi::Rights::all(),
    //                 fs_rights_inheriting: wasi::Rights::all(),
    //                 fs_flags: wasi::Fdflags::all(),
    //             },
    //             JournalEntry::FileDescriptorWriteV1 {
    //                 fd: 1235,
    //                 offset: 1234,
    //                 data: [6u8; 16].to_vec().into(),
    //                 is_64bit: true,
    //             },
    //             JournalEntry::CloseFileDescriptorV1 { fd: 1235 },
    //         ],
    //         vec![
    //             JournalEntry::OpenFileDescriptorV1 {
    //                 fd: 1235,
    //                 dirfd: 3452345,
    //                 dirflags: 0,
    //                 path: "/blah".into(),
    //                 o_flags: wasi::Oflags::CREATE | wasi::Oflags::TRUNC,
    //                 fs_rights_base: wasi::Rights::all(),
    //                 fs_rights_inheriting: wasi::Rights::all(),
    //                 fs_flags: wasi::Fdflags::all(),
    //             },
    //             JournalEntry::FileDescriptorWriteV1 {
    //                 fd: 1235,
    //                 offset: 1234,
    //                 data: [6u8; 16].to_vec().into(),
    //                 is_64bit: true,
    //             },
    //             JournalEntry::CloseFileDescriptorV1 { fd: 1235 },
    //         ],
    //     )
    //     .unwrap()
    // }
    //
    // #[tracing_test::traced_test]
    // #[test]
    // pub fn test_compact_file_system_ignore_double_writes() {
    //     run_test(
    //         vec![
    //             JournalEntry::OpenFileDescriptorV1 {
    //                 fd: 1234,
    //                 dirfd: 3452345,
    //                 dirflags: 0,
    //                 path: "/blah".into(),
    //                 o_flags: wasi::Oflags::empty(),
    //                 fs_rights_base: wasi::Rights::all(),
    //                 fs_rights_inheriting: wasi::Rights::all(),
    //                 fs_flags: wasi::Fdflags::all(),
    //             },
    //             JournalEntry::FileDescriptorWriteV1 {
    //                 fd: 1234,
    //                 offset: 1234,
    //                 data: [1u8; 16].to_vec().into(),
    //                 is_64bit: true,
    //             },
    //             JournalEntry::FileDescriptorWriteV1 {
    //                 fd: 1234,
    //                 offset: 1234,
    //                 data: [5u8; 16].to_vec().into(),
    //                 is_64bit: true,
    //             },
    //             JournalEntry::CloseFileDescriptorV1 { fd: 1234 },
    //         ],
    //         vec![
    //             JournalEntry::OpenFileDescriptorV1 {
    //                 fd: 1234,
    //                 dirfd: 3452345,
    //                 dirflags: 0,
    //                 path: "/blah".into(),
    //                 o_flags: wasi::Oflags::empty(),
    //                 fs_rights_base: wasi::Rights::all(),
    //                 fs_rights_inheriting: wasi::Rights::all(),
    //                 fs_flags: wasi::Fdflags::all(),
    //             },
    //             JournalEntry::FileDescriptorWriteV1 {
    //                 fd: 1234,
    //                 offset: 1234,
    //                 data: [5u8; 16].to_vec().into(),
    //                 is_64bit: true,
    //             },
    //             JournalEntry::CloseFileDescriptorV1 { fd: 1234 },
    //         ],
    //     )
    //     .unwrap()
    // }
    //
    // #[tracing_test::traced_test]
    // #[test]
    // pub fn test_compact_file_system_create_directory() {
    //     run_test(
    //         vec![JournalEntry::CreateDirectoryV1 {
    //             fd: 1234,
    //             path: "/blah".into(),
    //         }],
    //         vec![JournalEntry::CreateDirectoryV1 {
    //             fd: 1234,
    //             path: "/blah".into(),
    //         }],
    //     )
    //     .unwrap()
    // }
    //
    // #[tracing_test::traced_test]
    // #[test]
    // pub fn test_compact_file_system_redundant_create_directory() {
    //     run_test(
    //         vec![
    //             JournalEntry::CreateDirectoryV1 {
    //                 fd: 1234,
    //                 path: "/blah".into(),
    //             },
    //             JournalEntry::CreateDirectoryV1 {
    //                 fd: 1235,
    //                 path: "/blah".into(),
    //             },
    //         ],
    //         vec![JournalEntry::CreateDirectoryV1 {
    //             fd: 1234,
    //             path: "/blah".into(),
    //         }],
    //     )
    //     .unwrap()
    // }
    //
    // #[tracing_test::traced_test]
    // #[test]
    // pub fn test_compact_duplicate_tty() {
    //     run_test(
    //         vec![
    //             JournalEntry::TtySetV1 {
    //                 tty: Tty {
    //                     cols: 123,
    //                     rows: 65,
    //                     width: 2341,
    //                     height: 573457,
    //                     stdin_tty: true,
    //                     stdout_tty: true,
    //                     stderr_tty: true,
    //                     echo: true,
    //                     line_buffered: true,
    //                 },
    //                 line_feeds: true,
    //             },
    //             JournalEntry::TtySetV1 {
    //                 tty: Tty {
    //                     cols: 12,
    //                     rows: 65,
    //                     width: 2341,
    //                     height: 573457,
    //                     stdin_tty: true,
    //                     stdout_tty: false,
    //                     stderr_tty: true,
    //                     echo: true,
    //                     line_buffered: true,
    //                 },
    //                 line_feeds: true,
    //             },
    //         ],
    //         vec![JournalEntry::TtySetV1 {
    //             tty: Tty {
    //                 cols: 12,
    //                 rows: 65,
    //                 width: 2341,
    //                 height: 573457,
    //                 stdin_tty: true,
    //                 stdout_tty: false,
    //                 stderr_tty: true,
    //                 echo: true,
    //                 line_buffered: true,
    //             },
    //             line_feeds: true,
    //         }],
    //     )
    //     .unwrap()
    // }

    #[tracing_test::traced_test]
    #[test]
    pub fn test_compact_close_sockets() {
        let fd = 512;
        run_test(
            vec![
                JournalEntry::SocketConnectedV1 {
                    fd,
                    local_addr: "127.0.0.1:3333".parse().unwrap(),
                    peer_addr: "127.0.0.1:9999".parse().unwrap(),
                },
                JournalEntry::SocketSendV1 {
                    fd,
                    data: Cow::Borrowed(b"123"),
                    // flags: SiFlags,
                    flags: Default::default(),
                    is_64bit: false,
                },
                JournalEntry::SocketSendV1 {
                    fd,
                    data: Cow::Borrowed(b"123"),
                    // flags: SiFlags,
                    flags: Default::default(),
                    is_64bit: false,
                },
                JournalEntry::SocketSendV1 {
                    fd,
                    data: Cow::Borrowed(b"456"),
                    // flags: SiFlags,
                    flags: Default::default(),
                    is_64bit: false,
                },
                JournalEntry::CloseFileDescriptorV1 { fd: 512 },
            ],
            vec![],
        )
        .unwrap()
    }
}
