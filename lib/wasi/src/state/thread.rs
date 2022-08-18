use std::{
    sync::{
        Mutex,
        Arc,
        Condvar, RwLock, atomic::{AtomicU32, Ordering}, RwLockWriteGuard, RwLockReadGuard
    },
    time::Duration, collections::{HashMap, HashSet}, borrow::Cow, ops::{Deref, DerefMut}
};

use bytes::{BytesMut, Bytes};
use tracing::log::trace;
use wasmer_vbus::{BusSpawnedProcess, SignalHandlerAbi};
use wasmer_wasi_types::{__wasi_signal_t, __wasi_exitcode_t, __WASI_CLOCK_MONOTONIC, __wasi_errno_t, __WASI_ECHILD};

use crate::syscalls::platform_clock_time_get;

/// Represents the ID of a WASI thread
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WasiThreadId(u32);

impl WasiThreadId {
    pub fn raw(&self) -> u32 {
        self.0
    }

    pub fn inc(&mut self) -> WasiThreadId {
        let ret = self.clone();
        self.0 += 1;
        ret
    }
}

impl From<u32> for WasiThreadId {
    fn from(id: u32) -> Self {
        Self(id)
    }
}
impl From<WasiThreadId> for u32 {
    fn from(t: WasiThreadId) -> u32 {
        t.0 as u32
    }
}

impl std::fmt::Display
for WasiThreadId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Represents a linked list of stack snapshots
#[derive(Debug, Clone)]
struct ThreadSnapshot
{
    call_stack: Bytes,
    store_data: Bytes,
}

/// Represents a linked list of stack snapshots
#[derive(Debug, Clone, Default)]
struct ThreadStack
{
    memory_stack: Vec<u8>,
    memory_stack_corrected: Vec<u8>,
    snapshots: HashMap<u128, ThreadSnapshot>,
    next: Option<Box<ThreadStack>>,
}

/// Represents a running thread which allows a joiner to
/// wait for the thread to exit
#[derive(Debug, Clone)]
pub struct WasiThread
{
    pub(crate) is_main: bool,
    pub(crate) pid: WasiProcessId,
    pub(crate) id: WasiThreadId,
    finished: Arc<(Mutex<Option<u32>>, Condvar)>,
    pub(crate) signals: Arc<(Mutex<Vec<__wasi_signal_t>>, tokio::sync::broadcast::Sender<()>)>,
    stack: Arc<Mutex<ThreadStack>>,
}

impl WasiThread
{
    /// Returns the process ID
    pub fn pid(&self) -> WasiProcessId {
        self.pid
    }

    /// Returns the thread ID
    pub fn tid(&self) -> WasiThreadId {
        self.id
    }

    /// Returns true if this thread is the main thread
    pub fn is_main(&self) -> bool {
        self.is_main
    }

    /// Marks the thread as finished (which will cause anyone that
    /// joined on it to wake up)
    pub fn terminate(&self, exit_code: u32) {
        let mut guard = self.finished.0.lock().unwrap();
        if guard.is_none() {
            *guard = Some(exit_code);
        }
        self.finished.1.notify_all();
    }

    /// Waits until the thread is finished or the timeout is reached
    pub fn join(&self, timeout: Duration) -> Option<__wasi_exitcode_t> {
        let mut finished = self.finished.0.lock().unwrap();
        if finished.is_some() {
            return finished.clone();
        }
        loop {
            let woken = self.finished.1.wait_timeout(finished, timeout).unwrap();
            if woken.1.timed_out() {
                return None;
            }
            finished = woken.0;
            if finished.is_some() {
                return finished.clone();
            }
        }
    }

    /// Attempts to join on the thread
    pub fn try_join(&self) -> Option<__wasi_exitcode_t> {
        let guard = self.finished.0.lock().unwrap();
        guard.clone()
    }

    /// Adds a signal for this thread to process
    pub fn signal(&self, signal: __wasi_signal_t) {
        let mut guard = self.signals.0.lock().unwrap();
        if guard.contains(&signal) == false {
            guard.push(signal);
        }
        let _ = self.signals.1.send(());
    }

    /// Returns all the signals that are waiting to be processed
    pub fn pop_signals(&self) -> Vec<__wasi_signal_t> {
        let mut guard = self.signals.0.lock().unwrap();
        guard.drain(..).collect()
    }

    pub fn subscribe_signals(&self) -> tokio::sync::broadcast::Receiver<()> {
        self.signals.1.subscribe()
    }

    /// Adds a stack snapshot and removes dead ones
    pub fn add_snapshot(&self, mut memory_stack: &[u8], memory_stack_corrected: &[u8], hash: u128, rewind_stack: &[u8], store_data: &[u8]) {
        // Lock the stack
        let mut stack = self.stack.lock().unwrap();
        let mut pstack = stack.deref_mut();
        loop {
            // First we validate if the stack is no longer valid
            let memory_stack_before = pstack.memory_stack.len();
            let memory_stack_after= memory_stack.len();
            if memory_stack_before > memory_stack_after ||
                (
                    pstack.memory_stack.iter().zip(memory_stack.iter()).any(|(a, b)| *a == *b) == false &&
                    pstack.memory_stack_corrected.iter().zip(memory_stack.iter()).any(|(a, b)| *a == *b) == false
                )
            {
                // The stacks have changed so need to start again at this segment
                let mut new_stack = ThreadStack::default();
                new_stack.memory_stack = memory_stack.to_vec();
                new_stack.memory_stack_corrected = memory_stack_corrected.to_vec();
                std::mem::swap(pstack, &mut new_stack);
                memory_stack = &memory_stack[memory_stack.len()..];

                // Output debug info for the dead stack
                let mut disown = Some(Box::new(new_stack));
                if disown.is_some() {
                    tracing::trace!("wasi[{}]::stacks forgotten (memory_stack_before={}, memory_stack_after={})", self.pid, memory_stack_before, memory_stack_after);
                }
                while let Some(disowned) = disown {
                    for hash in disowned.snapshots.keys() {
                        tracing::trace!("wasi[{}]::stack has been forgotten (hash={})", self.pid, hash);
                    }
                    disown = disowned.next;
                }
            } else {
                memory_stack = &memory_stack[pstack.memory_stack.len()..];
            }

            // If there is no more memory stack then we are done and can add the call stack
            if memory_stack.len() <= 0 {
                break;
            }

            // Otherwise we need to add a next stack pointer and continue the iterations
            if pstack.next.is_none() {
                let mut new_stack = ThreadStack::default();
                new_stack.memory_stack = memory_stack.to_vec();
                pstack.next.replace(Box::new(new_stack));
            }
            pstack = pstack.next.as_mut().unwrap();
        }

        // Add the call stack
        pstack.snapshots.insert(hash, ThreadSnapshot {
            call_stack: BytesMut::from(rewind_stack).freeze(),
            store_data: BytesMut::from(store_data).freeze(),
        });
    }

    /// Gets a snapshot that was previously addedf
    pub fn get_snapshot(&self, hash: u128) -> Option<(BytesMut, Bytes, Bytes)> {
        let mut memory_stack = BytesMut::new();

        let stack = self.stack.lock().unwrap();
        let mut pstack = stack.deref();
        loop {
            memory_stack.extend(pstack.memory_stack_corrected.iter());
            if let Some(snapshot) = pstack.snapshots.get(&hash) {
                return Some((memory_stack, snapshot.call_stack.clone(), snapshot.store_data.clone()));
            }
            if let Some(next) = pstack.next.as_ref() {
                pstack = next.deref();
            } else {
                return None;
            }
        }
    }

    // Copy the stacks from another thread
    pub fn copy_stack_from(&self, other: &WasiThread) {
        let mut stack = {
            let stack_guard = other.stack.lock().unwrap();
            stack_guard.clone()
        };

        let mut stack_guard = self.stack.lock().unwrap();
        std::mem::swap(stack_guard.deref_mut(), &mut stack);
    }
}

#[derive(Debug, Clone)]
pub struct WasiThreadHandle {
    id: Arc<WasiThreadId>,
    thread: WasiThread,
    inner: Arc<RwLock<WasiProcessInner>>,
}

impl WasiThreadHandle {
    pub fn id(&self) -> WasiThreadId {
        self.id.0.into()
    }

    pub fn as_thread(&self) -> WasiThread {
        self.thread.clone()
    }
}

impl Drop
for WasiThreadHandle {
    fn drop(&mut self) {
        // We do this so we track when the last handle goes out of scope
        if let Some(id) = Arc::get_mut(&mut self.id) {
            let mut inner = self.inner.write().unwrap();
            if let Some(ctrl) = inner.threads.remove(id) {
                ctrl.terminate(0);
            }
            inner.thread_count -= 1;   
        }
    }
}

impl std::ops::Deref
for WasiThreadHandle {
    type Target = WasiThread;

    fn deref(&self) -> &Self::Target {
        &self.thread
    }
}

impl std::ops::DerefMut
for WasiThreadHandle {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.thread
    }
}

/// Represents the ID of a sub-process
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WasiProcessId(u32);

impl WasiProcessId {
    pub fn raw(&self) -> u32 {
        self.0
    }
}

impl From<i32> for WasiProcessId {
    fn from(id: i32) -> Self {
        Self(id as u32)
    }
}
impl Into<i32> for WasiProcessId {
    fn into(self) -> i32 {
        self.0 as i32
    }
}

impl From<u32> for WasiProcessId {
    fn from(id: u32) -> Self {
        Self(id)
    }
}
impl Into<u32> for WasiProcessId {
    fn into(self) -> u32 {
        self.0 as u32
    }
}

impl std::fmt::Display
for WasiProcessId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug)]
pub struct WasiSignalInterval
{
    /// Signal that will be raised
    pub signal: u8,
    /// Time between the signals
    pub interval: Duration,
    /// Flag that indicates if the signal should repeat
    pub repeat: bool,
    /// Last time that a signal was triggered
    pub last_signal: u128,
}

#[derive(Debug)]
pub struct WasiProcessInner
{
    /// The threads that make up this process
    pub threads: HashMap<WasiThreadId, WasiThread>,
    /// Number of threads running for this process
    pub thread_count: u32,
    /// Seed used to generate thread ID's
    pub thread_seed: WasiThreadId,
    /// All the thread local variables
    pub thread_local: HashMap<(WasiThreadId, u32), u64>,
    /// User data associated with thread local data
    pub thread_local_user_data: HashMap<u32, u64>,
    /// Seed used to generate thread locals
    pub thread_local_seed: u32,
    /// Signals that will be triggered at specific intervals
    pub signal_intervals: HashMap<u8, WasiSignalInterval>,
    /// Represents all the process spun up as a bus process
    pub bus_processes: HashMap<WasiProcessId, Box<BusSpawnedProcess>>,
    /// Indicates if the bus process can be reused
    pub bus_process_reuse: HashMap<Cow<'static, str>, WasiProcessId>,
}

/// Represents a process running within the compute state
#[derive(Debug, Clone)]
pub struct WasiProcess
{
    /// Unique ID of this process
    pub(crate) pid: WasiProcessId,
    /// ID of the parent process
    pub(crate) ppid: WasiProcessId,
    /// The inner protected region of the process
    pub(crate) inner: Arc<RwLock<WasiProcessInner>>,
    /// Reference back to the compute engine
    pub(crate) compute: WasiControlPlane,
    /// Reference to the exit code for the main thread
    pub(crate) finished: Arc<(Mutex<Option<u32>>, Condvar)>,
    /// List of all the children spawned from this thread
    pub(crate) children: Arc<RwLock<Vec<WasiProcessId>>>,
    /// Number of threads waiting for children to exit
    pub(crate) waiting: Arc<AtomicU32>,
}

pub(crate) struct WasiProcessWait {
    waiting: Arc<AtomicU32>,
}

impl WasiProcessWait {
    pub fn new(process: &WasiProcess) -> Self {
        process.waiting.fetch_add(1, Ordering::AcqRel);
        Self {
            waiting: process.waiting.clone()
        }
    }
}

impl Drop
for WasiProcessWait {
    fn drop(&mut self) {
        self.waiting.fetch_sub(1, Ordering::AcqRel);
    }
}

impl WasiProcess
{
    /// Gets the process ID of this process
    pub fn pid(&self) -> WasiProcessId {
        self.pid
    }

    /// Gets the process ID of the parent process
    pub fn ppid(&self) -> WasiProcessId {
        self.ppid
    }

    /// Gains write access to the process internals
    pub fn write(&self) -> RwLockWriteGuard<WasiProcessInner> {
        self.inner.write().unwrap()
    }

    /// Gains read access to the process internals
    pub fn read(&self) -> RwLockReadGuard<WasiProcessInner> {
        self.inner.read().unwrap()
    }

    /// Creates a a thread and returns it
    pub fn new_thread(&self) -> WasiThreadHandle {
        let mut inner = self.inner.write().unwrap();
        let id = inner.thread_seed.inc();

        let mut is_main = false;
        let finished = if inner.thread_count <= 0 {
            is_main = true;
            self.finished.clone()
        } else {
            Arc::new((Mutex::new(None), Condvar::default()))
        };

        let (tx_signals, _) = tokio::sync::broadcast::channel(1);
        let ctrl = WasiThread {
            pid: self.pid(),
            id,
            is_main,
            finished,
            signals: Arc::new((Mutex::new(Vec::new()), tx_signals)),
            stack: Arc::new(Mutex::new(ThreadStack::default()))
        };
        inner.threads.insert(id, ctrl.clone());
        inner.thread_count += 1;
        
        WasiThreadHandle {
            id: Arc::new(id),
            thread: ctrl,
            inner: self.inner.clone(),
        }
    }

    /// Gets a reference to a particular thread
    pub fn get_thread(&self, tid: &WasiThreadId) -> Option<WasiThread> {
        let inner = self.inner.read().unwrap();
        inner.threads.get(tid).map(|a| a.clone())
    }
    
    /// Signals a particular thread in the process
    pub fn signal_thread(&self, tid: &WasiThreadId, signal: __wasi_signal_t) {
        let inner = self.inner.read().unwrap();
        if let Some(thread) = inner.threads.get(tid) {
            thread.signal(signal);
        } else {
            trace!("wasi[{}]::lost-signal(tid={}, sig={})", self.pid(), tid.0, signal);
        }
    }

    /// Signals all the threads in this process
    pub fn signal_process(&self, signal: __wasi_signal_t) {
        if self.waiting.load(Ordering::Acquire) > 0 {
            let children = self.children.read().unwrap();
            for pid in children.iter() {
                if let Some(process) = self.compute.get_process(*pid) {
                    process.signal_process(signal);
                }
            }
            return;
        }
        let inner = self.inner.read().unwrap();
        for thread in inner.threads.values() {
            thread.signal(signal);
        }
    }

    /// Signals one of the threads every interval
    pub fn signal_interval(&self, signal: __wasi_signal_t, interval: Option<Duration>, repeat: bool) {
        let mut inner = self.inner.write().unwrap();

        let interval = match interval {
            None => {
                inner.signal_intervals.remove(&signal);
                return;
            },
            Some(a) => a,
        };

        let now = platform_clock_time_get(__WASI_CLOCK_MONOTONIC, 1_000_000).unwrap() as u128;
        inner.signal_intervals.insert(signal, 
            WasiSignalInterval {
                signal,
                interval,
                last_signal: now,
                repeat
            }
        );
    }

    /// Returns the number of active threads for this process
    pub fn active_threads(&self) -> u32 {
        let inner = self.inner.read().unwrap();
        inner.thread_count
    }

    /// Waits until the process is finished or the timeout is reached
    pub fn join(&self, timeout: Duration) -> Option<__wasi_exitcode_t> {
        let _guard = WasiProcessWait::new(self);
        let mut finished = self.finished.0.lock().unwrap();
        if finished.is_some() {
            return finished.clone();
        }
        loop {
            let woken = self.finished.1.wait_timeout(finished, timeout).unwrap();
            if woken.1.timed_out() {
                return None;
            }
            finished = woken.0;
            if finished.is_some() {
                return finished.clone();
            }
        }
    }

    /// Waits for all the children to be finished
    pub fn join_children(&mut self, timeout: Duration) -> Option<__wasi_exitcode_t> {
        let _guard = WasiProcessWait::new(self);
        let mut exit_code = 0;
        let children: Vec<_> = {
            let children = self.children.read().unwrap();
            children.clone()
        };
        for pid in children {
            if let Some(process) = self.compute.get_process(pid) {
                match process.join(timeout) {
                    Some(a) => {
                        let mut children = self.children.write().unwrap();
                        children.retain(|a| *a != pid);
                        exit_code = a;
                    },
                    None => {
                        return None;
                    }
                }
            }
        }
        Some(exit_code)
    }

    /// Waits for all the children to be finished
    pub fn join_any_child(&mut self, timeout: Duration) -> Result<Option<(WasiProcessId, __wasi_exitcode_t)>, __wasi_errno_t> {
        let _guard = WasiProcessWait::new(self);
        let children: Vec<_> = {
            let children = self.children.read().unwrap();
            children.clone()
        };
        if children.is_empty() {
            return Err(__WASI_ECHILD);
        }
        for pid in children {
            if let Some(process) = self.compute.get_process(pid) {
                if let Some(exit_code) = process.join(timeout) {
                    let pid = process.pid();
                    let mut children = self.children.write().unwrap();
                    children.retain(|a| *a != pid);
                    return Ok(Some((pid, exit_code)))
                }
            }
        }
        Ok(None)
    }

    /// Attempts to join on the process
    pub fn try_join(&self) -> Option<__wasi_exitcode_t> {
        let guard = self.finished.0.lock().unwrap();
        guard.clone()
    }

    /// Terminate the process and all its threads
    pub fn terminate(&self, exit_code: u32) {
        let guard = self.inner.read().unwrap();
        for thread in guard.threads.values() {
            thread.terminate(exit_code)
        }
    }

    /// Gains access to the compute control plane
    pub fn control_plane(&self) -> &WasiControlPlane {
        &self.compute
    }
}

impl SignalHandlerAbi
for WasiProcess {
    fn signal(&self, sig: __wasi_signal_t) {
        self.signal_process(sig)
    }
}

#[derive(Debug, Clone)]
pub struct WasiControlPlane
{
    /// The processes running on this machine
    pub(crate) processes: Arc<RwLock<HashMap<WasiProcessId, WasiProcess>>>,
    /// Seed used to generate process ID's
    pub(crate) process_seed: Arc<AtomicU32>,
    /// Allows for a PID to be reserved
    pub(crate) reserved: Arc<Mutex<HashSet<WasiProcessId>>>,
}

impl Default
for WasiControlPlane
{
    fn default() -> Self {
        Self {
            processes: Default::default(),
            process_seed: Arc::new(AtomicU32::new(0)),
            reserved: Default::default(),
        }
    }
}

impl WasiControlPlane
{
    /// Reserves a PID and returns it
    pub fn reserve_pid(&self) -> WasiProcessId {
        let mut pid: WasiProcessId;
        loop {
            pid = self.process_seed.fetch_add(1, Ordering::AcqRel).into();

            {
                let mut guard = self.reserved.lock().unwrap();
                if guard.contains(&pid) {
                    continue;
                }
                guard.insert(pid);
            }
            
            {
                let guard = self.processes.read().unwrap();
                if guard.contains_key(&pid) == false {
                    break;
                }
            }

            {
                let mut guard = self.reserved.lock().unwrap();
                guard.remove(&pid);
            }
        }
        pid
    }

    /// Creates a new process
    pub fn new_process(&self) -> WasiProcess {
        let pid = self.reserve_pid();
        let ret = WasiProcess {
            pid,
            ppid: 0u32.into(),
            compute: self.clone(),
            inner: Arc::new(RwLock::new(WasiProcessInner {
                threads: Default::default(),
                thread_count: Default::default(),
                thread_seed: Default::default(),
                thread_local: Default::default(),
                thread_local_user_data: Default::default(),
                thread_local_seed: Default::default(),
                signal_intervals: Default::default(),
                bus_processes: Default::default(),
                bus_process_reuse: Default::default() 
            })),
            children: Arc::new(RwLock::new(Default::default())),
            finished: Arc::new((Mutex::new(None), Condvar::default())),
            waiting: Arc::new(AtomicU32::new(0))
        };
        {
            let mut guard = self.processes.write().unwrap();
            guard.insert(pid, ret.clone());
        }
        {
            let mut guard = self.reserved.lock().unwrap();
            guard.remove(&pid);
        }
        ret
    }

    /// Gets a reference to a running process
    pub fn get_process(&self, pid: WasiProcessId) -> Option<WasiProcess> {
        let guard = self.processes.read().unwrap();
        guard.get(&pid).map(|a| a.clone())
    }
}
