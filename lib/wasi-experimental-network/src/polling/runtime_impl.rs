use crate::polling::types::*;
//use crate::runtime_impl::Error;
use crate::types::*;
use mio::{self, event::Source};
use slab::Slab;
use std::convert::TryInto;
use std::io;
#[cfg(target_family = "unix")]
use std::os::unix::io::RawFd;
use std::sync::{Arc, RwLock};
use wasmer::{Exports, Function, LazyInit, Memory, Store, WasmerEnv};
use wasmer_wasi::{ptr::WasmPtr, WasiEnv};

macro_rules! wasi_try {
    ($expr:expr) => {{
        let res: Result<_, __wasi_errno_t> = $expr;

        match res {
            Ok(val) => val,
            Err(err) => return err,
        }
    }};

    ($expr:expr, $e:expr) => {{
        let opt: Option<_> = $expr;
        wasi_try!(opt.ok_or($e))
    }};
}

fn poll_create(env: &WasiNetworkEnv, poll_id: WasmPtr<__wasi_poll_t>) -> __wasi_errno_t {
    let poll_index = env
        .poll_arena
        .try_write()
        .unwrap()
        .insert(mio::Poll::new().unwrap());

    assert!(poll_index < (u32::MAX as usize));

    let memory = env.memory();
    let poll_id_cell = wasi_try!(poll_id.deref(memory));

    poll_id_cell.set(poll_index as u32);

    __WASI_ESUCCESS
}

trait TryFrom<T>: Sized {
    type Error;
    fn try_from(value: T) -> Result<Self, Self::Error>;
}

impl TryFrom<__wasi_poll_interest_t> for mio::Interest {
    type Error = &'static str;

    fn try_from(value: __wasi_poll_interest_t) -> Result<Self, Self::Error> {
        let mut interest: Option<mio::Interest> = None;

        if (value & READABLE_INTEREST) != 0 {
            interest = Some(mio::Interest::READABLE);
        }

        if (value & WRITABLE_INTEREST) != 0 {
            interest = interest.map_or_else(
                || Some(mio::Interest::WRITABLE),
                |interest| Some(interest.add(mio::Interest::WRITABLE)),
            );
        }

        #[cfg(any(
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "ios",
            target_os = "macos"
        ))]
        if (value & AIO_INTEREST) != 0 {
            interest = interest.map_or_else(
                || Some(mio::Interest::AIO),
                |interest| Some(interest.add(mio::Interest::AIO)),
            );
        }

        #[cfg(target_os = "freebsd")]
        if (value & LIO_INTEREST) != 0 {
            interest = interest.map_or_else(
                || Some(mio::Interest::LIO),
                |interest| Some(interest.add(mio::Interest::LIO)),
            );
        }

        interest.ok_or_else(|| "`__wasi_poll_interest_t` contains unknown values")
    }
}

struct WasiFd(__wasi_fd_t);

#[cfg(target_family = "unix")]
impl mio::event::Source for WasiFd {
    fn register(
        &mut self,
        registry: &mio::Registry,
        token: mio::Token,
        interests: mio::Interest,
    ) -> io::Result<()> {
        mio::unix::SourceFd(&(self.0 as RawFd)).register(registry, token, interests)
    }

    fn reregister(
        &mut self,
        registry: &mio::Registry,
        token: mio::Token,
        interests: mio::Interest,
    ) -> io::Result<()> {
        mio::unix::SourceFd(&(self.0 as RawFd)).reregister(registry, token, interests)
    }

    fn deregister(&mut self, registry: &mio::Registry) -> io::Result<()> {
        mio::unix::SourceFd(&(self.0 as RawFd)).deregister(registry)
    }
}

#[cfg(target_family = "unix")]
fn poll_register(
    env: &WasiNetworkEnv,
    poll_id: __wasi_poll_t,
    fd: __wasi_fd_t,
    token: __wasi_poll_token_t,
    interest: __wasi_poll_interest_t,
) -> __wasi_errno_t {
    let mut poll_lock = env.poll_arena.try_write().unwrap();
    let poll = poll_lock.get_mut(poll_id as usize).unwrap();

    let mut io_source = WasiFd(fd);
    io_source
        .register(
            poll.registry(),
            mio::Token(token as usize),
            mio::Interest::try_from(interest).unwrap(),
        )
        .unwrap();

    __WASI_ESUCCESS
}

fn poll(
    env: &WasiNetworkEnv,
    poll_id: __wasi_poll_t,
    events_id: __wasi_poll_events_t,
    events_size: WasmPtr<u32>,
) -> __wasi_errno_t {
    let mut poll_lock = env.poll_arena.try_write().unwrap();
    let poll = poll_lock.get_mut(poll_id as usize).unwrap();

    let mut events_lock = env.events_arena.try_write().unwrap();
    let mut events = events_lock.get_mut(events_id as usize).unwrap();

    poll.poll(&mut events, None).unwrap();

    let memory = env.memory();
    let events_size_cell = wasi_try!(events_size.deref(memory));

    events_size_cell.set(events.iter().len().try_into().unwrap());

    __WASI_ESUCCESS
}

fn events_create(
    env: &WasiNetworkEnv,
    capacity: u32,
    events_id: WasmPtr<__wasi_poll_events_t>,
) -> __wasi_errno_t {
    let events_index = env
        .events_arena
        .try_write()
        .unwrap()
        .insert(mio::Events::with_capacity(capacity as usize));

    assert!(events_index < (u32::MAX as usize));

    let memory = env.memory();
    let events_id_cell = wasi_try!(events_id.deref(memory));
    events_id_cell.set(events_index as u32);

    __WASI_ESUCCESS
}

fn event_token(
    env: &WasiNetworkEnv,
    events_id: __wasi_poll_events_t,
    event_nth: __wasi_poll_event_t,
    token: WasmPtr<__wasi_poll_token_t>,
) -> __wasi_errno_t {
    let events_lock = env.events_arena.try_read().unwrap();
    let events = events_lock.get(events_id as usize).unwrap();

    let event = events.iter().nth(event_nth as usize).unwrap();

    dbg!(&event);

    let memory = env.memory();
    let token_cell = wasi_try!(token.deref(memory));

    token_cell.set(usize::from(event.token()).try_into().unwrap());

    __WASI_ESUCCESS
}

#[derive(Debug, Clone, WasmerEnv)]
pub struct WasiNetworkEnv {
    wasi_env: Arc<WasiEnv>,
    poll_arena: Arc<RwLock<Slab<mio::Poll>>>,
    events_arena: Arc<RwLock<Slab<mio::Events>>>,
    #[wasmer(export)]
    memory: LazyInit<Memory>,
}

impl WasiNetworkEnv {
    pub fn memory(&self) -> &Memory {
        self.memory_ref()
            .expect("Memory should be set on `WasiNetworkEnv` first")
    }
}

pub fn get_namespace(store: &Store, wasi_env: &WasiEnv) -> (&'static str, Exports) {
    let wasi_network_env = WasiNetworkEnv {
        wasi_env: Arc::new(wasi_env.clone()),
        poll_arena: Arc::new(RwLock::new(Slab::new())),
        events_arena: Arc::new(RwLock::new(Slab::new())),
        memory: LazyInit::new(),
    };
    let mut wasi_network_imports = Exports::new();
    wasi_network_imports.insert(
        "poll_create",
        Function::new_native_with_env(&store, wasi_network_env.clone(), poll_create),
    );
    #[cfg(target_family = "unix")]
    wasi_network_imports.insert(
        "poll_register",
        Function::new_native_with_env(&store, wasi_network_env.clone(), poll_register),
    );
    wasi_network_imports.insert(
        "poll",
        Function::new_native_with_env(&store, wasi_network_env.clone(), poll),
    );
    wasi_network_imports.insert(
        "events_create",
        Function::new_native_with_env(&store, wasi_network_env.clone(), events_create),
    );
    wasi_network_imports.insert(
        "event_token",
        Function::new_native_with_env(&store, wasi_network_env.clone(), event_token),
    );

    (
        "wasi_experimental_network_ext_unstable",
        wasi_network_imports,
    )
}
