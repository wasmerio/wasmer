use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    task::{Context, RawWaker, RawWakerVTable, Waker},
};

#[derive(Debug, Clone, Serialize, Deserialize, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InterestType {
    Readable,
    Writable,
    Closed,
    Error,
}

#[derive(Debug)]
pub struct WakerInterestHandler {
    set: HashSet<InterestType>,
    waker: Waker,
}
impl WakerInterestHandler {
    pub fn new(waker: &Waker) -> Box<Self> {
        Box::new(WakerInterestHandler {
            set: Default::default(),
            waker: waker.clone(),
        })
    }
}
impl InterestHandler for WakerInterestHandler {
    fn push_interest(&mut self, interest: InterestType) {
        self.set.insert(interest);
        self.waker.wake_by_ref();
    }

    fn pop_interest(&mut self, interest: InterestType) -> bool {
        self.set.remove(&interest)
    }

    fn has_interest(&self, interest: InterestType) -> bool {
        self.set.contains(&interest)
    }
}

#[derive(Debug, Clone)]
pub struct SharedWakerInterestHandler {
    inner: Arc<Mutex<Box<WakerInterestHandler>>>,
}
impl SharedWakerInterestHandler {
    pub fn new(waker: &Waker) -> Box<Self> {
        Box::new(Self {
            inner: Arc::new(Mutex::new(WakerInterestHandler::new(waker))),
        })
    }
}
impl InterestHandler for SharedWakerInterestHandler {
    fn push_interest(&mut self, interest: InterestType) {
        let mut inner = self.inner.lock().unwrap();
        inner.push_interest(interest);
    }

    fn pop_interest(&mut self, interest: InterestType) -> bool {
        let mut inner = self.inner.lock().unwrap();
        inner.pop_interest(interest)
    }

    fn has_interest(&self, interest: InterestType) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.has_interest(interest)
    }
}

pub trait InterestHandler: Send + Sync + std::fmt::Debug {
    fn push_interest(&mut self, interest: InterestType);

    fn pop_interest(&mut self, interest: InterestType) -> bool;

    fn has_interest(&self, interest: InterestType) -> bool;
}

impl From<&Waker> for Box<dyn InterestHandler + Send + Sync> {
    fn from(waker: &Waker) -> Self {
        WakerInterestHandler::new(waker)
    }
}

impl From<&Context<'_>> for Box<dyn InterestHandler + Send + Sync> {
    fn from(cx: &Context) -> Self {
        cx.waker().into()
    }
}

pub fn handler_into_waker(
    handler: Box<dyn InterestHandler + Send + Sync>,
    interest: InterestType,
) -> Arc<InterestHandlerWaker> {
    Arc::new(InterestHandlerWaker {
        handler: Arc::new(Mutex::new(handler)),
        interest,
    })
}

#[derive(Debug, Clone)]
pub struct InterestHandlerWaker {
    handler: Arc<Mutex<Box<dyn InterestHandler + Send + Sync>>>,
    interest: InterestType,
}
impl InterestHandlerWaker {
    pub fn wake_now(&self) {
        let mut handler = self.handler.lock().unwrap();
        handler.push_interest(self.interest);
    }
    pub fn set_interest(self: &Arc<Self>, interest: InterestType) -> Arc<Self> {
        let mut next = self.as_ref().clone();
        next.interest = interest;
        Arc::new(next)
    }
    pub fn as_waker(self: &Arc<Self>) -> Waker {
        let s: *const Self = Arc::into_raw(Arc::clone(self));
        let raw_waker = RawWaker::new(s as *const (), &VTABLE);
        unsafe { Waker::from_raw(raw_waker) }
    }
}

fn handler_waker_wake(s: &InterestHandlerWaker) {
    let waker_arc = unsafe { Arc::from_raw(s) };
    waker_arc.wake_now();
}

fn handler_waker_clone(s: &InterestHandlerWaker) -> RawWaker {
    let arc = unsafe { Arc::from_raw(s) };
    std::mem::forget(arc.clone());
    RawWaker::new(Arc::into_raw(arc) as *const (), &VTABLE)
}

const VTABLE: RawWakerVTable = unsafe {
    RawWakerVTable::new(
        |s| handler_waker_clone(&*(s as *const InterestHandlerWaker)), // clone
        |s| handler_waker_wake(&*(s as *const InterestHandlerWaker)),  // wake
        |s| (*(s as *const InterestHandlerWaker)).wake_now(), // wake by ref (don't decrease refcount)
        |s| drop(Arc::from_raw(s as *const InterestHandlerWaker)), // decrease refcount
    )
};

#[derive(Debug, Clone, Default)]
struct InterestWakerMapState {
    wakers: HashMap<InterestType, Vec<Waker>>,
    triggered: HashSet<InterestType>,
}

#[derive(Debug, Clone, Default)]
pub struct InterestWakerMap {
    state: Arc<Mutex<InterestWakerMapState>>,
}

impl InterestWakerMap {
    pub fn add(&self, interest: InterestType, waker: &Waker) {
        let mut state = self.state.lock().unwrap();
        let entries = state.wakers.entry(interest).or_default();
        if !entries.iter().any(|w| w.will_wake(waker)) {
            entries.push(waker.clone());
        }
    }

    pub fn pop(&self, interest: InterestType) -> bool {
        let mut state = self.state.lock().unwrap();
        state.triggered.remove(&interest)
    }

    pub fn push(&self, interest: InterestType) -> bool {
        let mut state = self.state.lock().unwrap();
        state.triggered.insert(interest)
    }
}

impl InterestHandler for InterestWakerMap {
    fn push_interest(&mut self, interest: InterestType) {
        let mut state = self.state.lock().unwrap();
        if let Some(wakers) = state.wakers.remove(&interest) {
            for waker in wakers {
                waker.wake();
            }
        } else {
            state.triggered.insert(interest);
        }
    }

    fn pop_interest(&mut self, interest: InterestType) -> bool {
        let mut state = self.state.lock().unwrap();
        state.triggered.remove(&interest)
    }

    fn has_interest(&self, interest: InterestType) -> bool {
        let state = self.state.lock().unwrap();
        state.triggered.contains(&interest)
    }
}
