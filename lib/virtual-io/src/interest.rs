use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    task::{Context, RawWaker, RawWakerVTable, Waker},
};

use derivative::Derivative;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InterestType {
    Readable,
    Writable,
    Closed,
    Error,
}

pub trait InterestHandler: Send + Sync {
    fn interest(&mut self, interest: InterestType);
}

impl From<&Waker> for Box<dyn InterestHandler + Send + Sync> {
    fn from(waker: &Waker) -> Self {
        struct WakerHandler {
            waker: Waker,
        }
        impl InterestHandler for WakerHandler {
            fn interest(&mut self, _: InterestType) {
                self.waker.wake_by_ref();
            }
        }
        Box::new(WakerHandler {
            waker: waker.clone(),
        })
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

#[derive(Derivative, Clone)]
#[derivative(Debug)]
pub struct InterestHandlerWaker {
    #[derivative(Debug = "ignore")]
    handler: Arc<Mutex<Box<dyn InterestHandler + Send + Sync>>>,
    interest: InterestType,
}
impl InterestHandlerWaker {
    pub fn wake_now(&self) {
        let mut handler = self.handler.lock().unwrap();
        handler.interest(self.interest);
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

pub struct FilteredHandler {
    handler: Box<dyn InterestHandler + Send + Sync>,
    types: HashSet<InterestType>,
}

impl FilteredHandler {
    pub fn new(handler: Box<dyn InterestHandler + Send + Sync>) -> Box<Self> {
        Box::new(Self {
            handler,
            types: HashSet::default(),
        })
    }
    pub fn add_interest(mut self: Box<Self>, interest: InterestType) -> Box<Self> {
        self.types.insert(interest);
        self
    }
}

impl InterestHandler for FilteredHandler {
    fn interest(&mut self, interest: InterestType) {
        if self.types.contains(&interest) {
            self.handler.interest(interest);
        }
    }
}
