//! Files in flight in `SCM_RIGHTS` messages and the collection of Unix sockets that only such
//! messages keep alive (`net/unix/garbage.c`).
//!
//! - Each queued message's files are charged to the sender's user (`user->unix_inflight`),
//!   which `too_many_unix_fds` compares with the sender's `RLIMIT_NOFILE`.
//! - The Unix sockets among them are vertices of a graph whose edges are the messages: a
//!   socket in flight in another socket's receive queue (or in the queue of a connection a
//!   listener has not accepted yet) can be reached through that socket.
//! - A socket whose every reference is a message in flight is a candidate. Collection is the
//!   mark and sweep `unix_gc` made until Linux 6.10 replaced it with an SCC walk that finds the
//!   same sockets: candidates referenced from outside the candidates are live, so is everything
//!   their queues hold, and the receive queues of the candidates left are purged, which drops
//!   the last references to them.
//!
//! Collections run when a Unix socket is released while sockets are in flight, and when a
//! sender has many files in flight (`unix_schedule_gc`). The graph is read without holding the
//! registry while socket queues are locked; a collection that saw the in-flight sockets change
//! (a send, a receive, a peek copying files out) is abandoned for the next one rather than
//! acting on a torn view. Nothing makes a socket judged unreachable reachable again: only a
//! receive from a reachable socket hands out a reference.

use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
};

use super::{Msg, Sock, UnixFile};
use crate::file::OpenFile;
use crate::sync::MutexExt;

/// `UNIX_INFLIGHT_SANE_USER`: a sender with more files in flight than this collects before
/// sending more Unix sockets.
pub(super) const SANE_USER: usize = super::SCM_MAX_FD * 8;
/// How often one collection retries after the graph changed under it.
const ATTEMPTS: usize = 4;

/// The files in flight of a network namespace's Unix sockets.
#[derive(Default)]
pub(crate) struct Inflight {
    state: Mutex<State>,
    /// Held by the collection running; a release during it asks for another round.
    gc: Mutex<()>,
    again: AtomicBool,
}

#[derive(Default)]
struct State {
    /// `user->unix_inflight`, by real uid.
    users: HashMap<u32, usize>,
    /// The Unix sockets in flight (`gc_inflight_list`), by open file, with the number of
    /// messages holding each.
    sockets: HashMap<usize, (Weak<OpenFile>, usize)>,
    /// Bumped whenever the sockets in flight change or a peek copies some out.
    generation: u64,
}

fn key(file: &Arc<OpenFile>) -> usize {
    Arc::as_ptr(file) as usize
}

fn unix_socket(file: &OpenFile) -> Option<Arc<Sock>> {
    file.downcast::<UnixFile>().map(|unix| unix.0.clone())
}

impl Inflight {
    /// `user->unix_inflight` of `uid`.
    pub(crate) fn user(&self, uid: u32) -> usize {
        self.state
            .lock_or_recover()
            .users
            .get(&uid)
            .copied()
            .unwrap_or(0)
    }

    /// Whether any Unix socket is in flight, so that a release may have left a cycle.
    fn has_sockets(&self) -> bool {
        !self.state.lock_or_recover().sockets.is_empty()
    }

    /// `unix_add_edges`: `files` were queued in a message from `uid`.
    fn add(&self, files: &[Arc<OpenFile>], uid: u32) {
        let mut st = self.state.lock_or_recover();
        *st.users.entry(uid).or_default() += files.len();
        let mut changed = false;
        for file in files.iter().filter(|f| unix_socket(f).is_some()) {
            st.sockets
                .entry(key(file))
                .or_insert_with(|| (Arc::downgrade(file), 0))
                .1 += 1;
            changed = true;
        }
        st.generation += changed as u64;
    }

    /// `unix_del_edges`: the message holding `files` was received or dropped.
    fn remove(&self, files: &[Arc<OpenFile>], uid: u32) {
        if files.is_empty() {
            return;
        }
        let mut st = self.state.lock_or_recover();
        if let Some(n) = st.users.get_mut(&uid) {
            *n -= files.len();
            if *n == 0 {
                st.users.remove(&uid);
            }
        }
        let mut changed = false;
        for file in files.iter().filter(|f| unix_socket(f).is_some()) {
            if let Some(vertex) = st.sockets.get_mut(&key(file)) {
                vertex.1 -= 1;
                if vertex.1 == 0 {
                    st.sockets.remove(&key(file));
                }
                changed = true;
            }
        }
        st.generation += changed as u64;
    }

    /// `unix_peek_fpl`: a peek copied `files` out of a queue, references the graph does not
    /// show.
    pub(super) fn peeked(&self, files: &[Arc<OpenFile>]) {
        if files.iter().any(|f| unix_socket(f).is_some()) {
            self.state.lock_or_recover().generation += 1;
        }
    }

    /// `unix_schedule_gc(NULL)` from a release: collects if anything is in flight.
    pub(super) fn released(&self) {
        if self.has_sockets() {
            self.collect();
        }
    }

    /// `unix_gc`. A collection already running (this may be called from inside one, as the
    /// purged messages release sockets) is asked to go round once more instead.
    pub(crate) fn collect(&self) {
        let Ok(_running) = self.gc.try_lock() else {
            self.again.store(true, Ordering::Release);
            return;
        };
        loop {
            self.again.store(false, Ordering::Release);
            // Dropped while still holding `gc`: the releases they cause only ask for a round.
            let hitlist = (0..ATTEMPTS).find_map(|_| self.walk());
            drop(hitlist);
            if !self.again.load(Ordering::Acquire) {
                return;
            }
        }
    }

    /// One mark and sweep. Returns the purged messages, or `None` if the in-flight sockets
    /// changed while the graph was read.
    fn walk(&self) -> Option<Vec<Msg>> {
        let (generation, vertices) = {
            let st = self.state.lock_or_recover();
            let vertices: Vec<_> = st
                .sockets
                .iter()
                .map(|(&k, (file, n))| (k, file.clone(), *n))
                .collect();
            (st.generation, vertices)
        };
        // Candidates: every reference to the file is a message in flight.
        let candidates: HashMap<usize, (usize, Arc<OpenFile>, Arc<Sock>)> = vertices
            .into_iter()
            .filter(|(_, file, n)| file.strong_count() == *n)
            .filter_map(|(k, file, n)| {
                let file = file.upgrade()?;
                let sock = unix_socket(&file)?;
                Some((k, (n, file, sock)))
            })
            .collect();
        if candidates.is_empty() {
            return Some(Vec::new());
        }
        // `scan_children` with `dec_inflight`: the edges out of each candidate.
        let edges: HashMap<usize, Vec<usize>> = candidates
            .iter()
            .map(|(&k, (_, _, sock))| (k, sock.inflight_sockets()))
            .collect();
        let mut remaining: HashMap<usize, usize> =
            candidates.iter().map(|(&k, (n, ..))| (k, *n)).collect();
        for target in edges.values().flatten() {
            if let Some(n) = remaining.get_mut(target) {
                *n = n.saturating_sub(1);
            }
        }
        // `inc_inflight_move_tail`: what a candidate referenced from outside reaches is live.
        let mut live: HashSet<usize> = HashSet::new();
        let mut stack: Vec<usize> = remaining
            .iter()
            .filter(|(_, n)| **n > 0)
            .map(|(&k, _)| k)
            .collect();
        while let Some(k) = stack.pop() {
            if !live.insert(k) {
                continue;
            }
            for target in &edges[&k] {
                if candidates.contains_key(target) && !live.contains(target) {
                    stack.push(*target);
                }
            }
        }
        if self.state.lock_or_recover().generation != generation {
            return None;
        }
        let mut hitlist = Vec::new();
        for (k, (_, _, sock)) in &candidates {
            if !live.contains(k) {
                sock.purge(&mut hitlist);
            }
        }
        Some(hitlist)
    }
}

/// The files of one `SCM_RIGHTS` message while it sits in a queue, counted in [`Inflight`]
/// from queueing until received or dropped.
pub(super) struct InFlight {
    pub(super) files: Vec<Arc<OpenFile>>,
    /// The sender's user, charged for them.
    uid: u32,
    registry: Arc<Inflight>,
}

impl InFlight {
    pub(super) fn new(files: Vec<Arc<OpenFile>>, uid: u32, registry: &Arc<Inflight>) -> Self {
        registry.add(&files, uid);
        Self {
            files,
            uid,
            registry: registry.clone(),
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Hands the files to the receiver.
    pub(super) fn take(&mut self) -> Vec<Arc<OpenFile>> {
        let files = std::mem::take(&mut self.files);
        self.registry.remove(&files, self.uid);
        files
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        self.registry.remove(&self.files, self.uid);
    }
}
