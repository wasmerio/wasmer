use std::{
    collections::HashSet,
    task::{Context, Waker},
};

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
