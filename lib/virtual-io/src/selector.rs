use std::{
    collections::HashSet,
    mem::ManuallyDrop,
    sync::{
        mpsc::{Receiver, Sender},
        Arc, Mutex,
    },
};

use derivative::Derivative;
use mio::Token;

use crate::{HandlerWrapper, InterestType};

#[derive(Derivative)]
#[derivative(Debug)]
pub(crate) struct EngineInner {
    #[derivative(Debug = "ignore")]
    selector: mio::Poll,
    rx_drop: Receiver<Token>,
}

#[derive(Derivative)]
#[derivative(Debug)]
pub struct Selector {
    inner: Mutex<EngineInner>,
    #[derivative(Debug = "ignore")]
    pub(crate) registry: mio::Registry,
    pub(crate) tx_drop: Mutex<Sender<Token>>,
    closer: mio::Waker,
}

impl Selector {
    pub fn new() -> Arc<Self> {
        let (tx_drop, rx_drop) = std::sync::mpsc::channel();

        let selector = mio::Poll::new().unwrap();
        let engine = Arc::new(Selector {
            closer: mio::Waker::new(selector.registry(), Token(0)).unwrap(),
            registry: selector.registry().try_clone().unwrap(),
            inner: Mutex::new(EngineInner { selector, rx_drop }),
            tx_drop: Mutex::new(tx_drop),
        });

        {
            let engine = engine.clone();
            std::thread::spawn(move || {
                Self::run(engine);
            });
        }

        engine
    }

    pub fn shutdown(&self) {
        self.closer.wake().ok();
    }

    fn run(engine: Arc<Selector>) {
        // The outer loop is used to release the scope of the
        // read lock whenever it needs to do so
        let mut events = mio::Events::with_capacity(128);
        loop {
            let mut dropped = HashSet::new();

            {
                // Wait for an event to trigger
                let mut guard = engine.inner.lock().unwrap();
                guard.selector.poll(&mut events, None).unwrap();

                // Read all the tokens that have been destroyed
                while let Ok(token) = guard.rx_drop.try_recv() {
                    let s = token.0 as *mut HandlerWrapper;
                    drop(unsafe { Box::from_raw(s) });
                    dropped.insert(token);
                }
            }

            // Loop through all the events
            for event in events.iter() {
                // If the event is already dropped then ignore it
                let token = event.token();
                if dropped.contains(&token) {
                    continue;
                }

                // If its the close event then exit
                if token.0 == 0 {
                    return;
                }

                // Otherwise this is a waker we need to wake
                let s = event.token().0 as *mut HandlerWrapper;
                let mut handler = ManuallyDrop::new(unsafe { Box::from_raw(s) });
                if event.is_readable() {
                    tracing::trace!(token = ?token, interest = ?InterestType::Readable, "host epoll");
                    handler.0.interest(InterestType::Readable);
                }
                if event.is_writable() {
                    tracing::trace!(token = ?token, interest = ?InterestType::Writable, "host epoll");
                    handler.0.interest(InterestType::Writable);
                }
                if event.is_read_closed() || event.is_write_closed() {
                    tracing::trace!(token = ?token, interest = ?InterestType::Closed, "host epoll");
                    handler.0.interest(InterestType::Closed);
                }
                if event.is_error() {
                    tracing::trace!(token = ?token, interest = ?InterestType::Error, "host epoll");
                    handler.0.interest(InterestType::Error);
                }
            }
        }
    }
}
