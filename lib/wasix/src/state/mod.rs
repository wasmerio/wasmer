//! WARNING: the API exposed here is unstable and very experimental.  Certain things are not ready
//! yet and may be broken in patch releases.  If you're using this and have any specific needs,
//! please [let us know here](https://github.com/wasmerio/wasmer/issues/583) or by filing an issue.
//!
//! Wasmer always has a virtual root directory located at `/` at which all pre-opened directories can
//! be found.  It's possible to traverse between preopened directories this way as well (for example
//! `preopen-dir1/../preopen-dir2`).
//!
//! A preopened directory is a directory or directory + name combination passed into the
//! `generate_import_object` function.  These are directories that the caller has given
//! the WASI module permission to access.
//!
//! You can implement `VirtualFile` for your own types to get custom behavior and extend WASI, see the
//! [WASI plugin example](https://github.com/wasmerio/wasmer/blob/main/examples/plugin.rs).

#![allow(clippy::cognitive_complexity, clippy::too_many_arguments)]

mod builder;
pub mod context_switching;
mod env;
mod func_env;
mod handles;
mod linker;
mod types;

use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
    sync::Mutex,
    task::Waker,
    time::Duration,
};

#[cfg(feature = "enable-serde")]
use serde::{Deserialize, Serialize};
use wasmer_wasix_types::wasi::{
    Disposition, Errno, Fd as WasiFd, Rights, Signal, Snapshot0Clockid,
};

pub use self::{
    builder::*,
    env::{WasiEnv, WasiEnvInit, WasiModuleInstanceHandles, WasiModuleTreeHandles},
    func_env::WasiFunctionEnv,
    types::*,
};
use crate::{fs::WasiFs, syscalls::types::*, utils::WasiParkingLot};
pub(crate) use handles::*;
pub(crate) use linker::*;

/// all the rights enabled
pub const ALL_RIGHTS: Rights = Rights::all();

/// Represents a futex which will make threads wait for completion in a more
/// CPU efficient manner
#[derive(Debug, Default)]
pub struct WasiFutex {
    pub(crate) wakers: BTreeMap<u64, Option<Waker>>,
}

/// Structure that holds the state of BUS calls to this process and from
/// this process. BUS calls are the equivalent of RPC's with support
/// for all the major serializers
#[derive(Debug, Default)]
pub struct WasiBusState {
    poll_waker: WasiParkingLot,
}

impl WasiBusState {
    /// Gets a reference to the waker that can be used for
    /// asynchronous calls
    // TODO: review allow...
    #[allow(dead_code)]
    pub fn get_poll_waker(&self) -> Waker {
        self.poll_waker.get_waker()
    }

    /// Wakes one of the reactors thats currently waiting
    // TODO: review allow...
    #[allow(dead_code)]
    pub fn poll_wake(&self) {
        self.poll_waker.wake()
    }

    /// Will wait until either the reactor is triggered
    /// or the timeout occurs
    // TODO: review allow...
    #[allow(dead_code)]
    pub fn poll_wait(&self, timeout: Duration) -> bool {
        self.poll_waker.wait(timeout)
    }
}

/// Stores the state of the futexes
#[derive(Debug, Default)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub(crate) struct WasiFutexState {
    pub poller_seed: u64,
    pub futexes: HashMap<u64, WasiFutex>,
}

/// Top level data type containing all* the state with which WASI can
/// interact.
///
/// * The contents of files are not stored and may be modified by
///   other, concurrently running programs.  Data such as the contents
///   of directories are lazily loaded.
#[derive(Debug)]
#[cfg_attr(feature = "enable-serde", derive(Serialize, Deserialize))]
pub(crate) struct WasiState {
    pub secret: [u8; 32],

    pub fs: WasiFs,
    pub futexs: Mutex<WasiFutexState>,
    pub clock_offset: Mutex<HashMap<Snapshot0Clockid, i64>>,
    pub args: Mutex<Vec<String>>,
    pub envs: Mutex<Vec<Vec<u8>>>,
    pub signals: Mutex<HashMap<Signal, Disposition>>,

    // TODO: should not be here, since this requires active work to resolve.
    // State should only hold active runtime state that can be reproducibly re-created.
    pub preopen: Vec<String>,
}

impl WasiState {
    // fn new(fs: WasiFs, inodes: Arc<RwLock<WasiInodes>>) -> Self {
    //     WasiState {
    //         fs,
    //         secret: rand::thread_rng().gen::<[u8; 32]>(),
    //         inodes,
    //         args: Vec::new(),
    //         preopen: Vec::new(),
    //         threading: Default::default(),
    //         futexs: Default::default(),
    //         clock_offset: Default::default(),
    //         envs: Vec::new(),
    //     }
    // }
}

// Implementations of direct to FS calls so that we can easily change their implementation
impl WasiState {
    /// Turn the WasiState into bytes
    #[cfg(feature = "enable-serde")]
    pub fn freeze(&self) -> Option<Vec<u8>> {
        bincode::serialize(self).ok()
    }

    /// Get a WasiState from bytes
    #[cfg(feature = "enable-serde")]
    pub fn unfreeze(bytes: &[u8]) -> Option<Self> {
        bincode::deserialize(bytes).ok()
    }

    /// Forking the WasiState is used when either fork or vfork is called
    pub fn fork(&self) -> Self {
        WasiState {
            fs: self.fs.fork(),
            secret: self.secret,
            futexs: Default::default(),
            clock_offset: Mutex::new(self.clock_offset.lock().unwrap().clone()),
            args: Mutex::new(self.args.lock().unwrap().clone()),
            envs: Mutex::new(self.envs.lock().unwrap().clone()),
            signals: Mutex::new(self.signals.lock().unwrap().clone()),
            preopen: self.preopen.clone(),
        }
    }
}
