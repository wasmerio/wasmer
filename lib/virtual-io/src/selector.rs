use std::{
    collections::HashMap,
    io,
    sync::{Arc, Mutex},
};

use mio::{Registry, Token};

use crate::{InterestHandler, InterestType};

#[derive(Debug)]
pub(crate) struct EngineInner {
    seed: usize,
    registry: Registry,
    lookup: HashMap<Token, Box<dyn InterestHandler + Send + Sync>>,
}

#[derive(Debug)]
pub struct Selector {
    token_close: Token,
    inner: Mutex<EngineInner>,
    closer: mio::Waker,
}

impl Selector {
    pub fn new() -> Arc<Self> {
        let poll = mio::Poll::new().unwrap();
        let registry = poll
            .registry()
            .try_clone()
            .expect("the selector registry failed to clone");

        let engine = Arc::new(Selector {
            closer: mio::Waker::new(poll.registry(), Token(0)).unwrap(),
            token_close: Token(1),
            inner: Mutex::new(EngineInner {
                seed: 10,
                lookup: Default::default(),
                registry,
            }),
        });

        {
            let engine = engine.clone();
            std::thread::spawn(move || {
                Self::run(engine, poll);
            });
        }

        engine
    }

    pub fn shutdown(&self) {
        self.closer.wake().ok();
    }

    #[must_use = "the token must be consumed"]
    pub fn add(
        &self,
        handler: Box<dyn InterestHandler + Send + Sync>,
        source: &mut dyn mio::event::Source,
        interests: mio::Interest,
    ) -> io::Result<Token> {
        let mut guard = self.inner.lock().unwrap();

        guard.seed = guard
            .seed
            .checked_add(1)
            .expect("selector has ran out of token seeds");
        let token = guard.seed;
        let token = Token(token);
        guard.lookup.insert(token, handler);

        match source.register(&guard.registry, token, interests) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                source.deregister(&guard.registry).ok();
                source.register(&guard.registry, token, interests)?;
            }
            Err(err) => return Err(err),
        };

        Ok(token)
    }

    pub fn remove(
        &self,
        token: Token,
        source: Option<&mut dyn mio::event::Source>,
    ) -> io::Result<()> {
        let mut guard = self.inner.lock().unwrap();
        guard.lookup.remove(&token);

        if let Some(source) = source {
            guard.registry.deregister(source)?;
        }
        Ok(())
    }

    pub fn handle<F>(&self, token: Token, f: F)
    where
        F: Fn(&mut Box<dyn InterestHandler + Send + Sync>),
    {
        let mut guard = self.inner.lock().unwrap();
        if let Some(handler) = guard.lookup.get_mut(&token) {
            f(handler)
        }
    }

    pub fn replace(&self, token: Token, mut handler: Box<dyn InterestHandler + Send + Sync>) {
        let mut guard = self.inner.lock().unwrap();

        let last = guard.lookup.remove(&token);
        if let Some(last) = last {
            let interests = vec![
                InterestType::Readable,
                InterestType::Writable,
                InterestType::Closed,
                InterestType::Error,
            ];
            for interest in interests {
                if last.has_interest(interest) && !handler.has_interest(interest) {
                    handler.push_interest(interest);
                }
            }
        }

        guard.lookup.insert(token, handler);
    }

    fn run(engine: Arc<Selector>, mut poll: mio::Poll) {
        // The outer loop is used to release the scope of the
        // read lock whenever it needs to do so
        let mut events = mio::Events::with_capacity(128);
        loop {
            // Wait for an event to trigger
            if let Err(e) = poll.poll(&mut events, None) {
                // This can happen when a debugger is attached
                #[cfg(debug_assertions)]
                if e.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                panic!("Unexpected error in selector poll loop: {e:?}");
            }

            // Loop through all the events while under a guard lock
            let mut guard = engine.inner.lock().unwrap();
            for event in events.iter() {
                // If the event is already dropped then ignore it
                let token = event.token();

                // If its the close event then exit
                if token == engine.token_close {
                    return;
                }

                // Get the handler
                let handler = match guard.lookup.get_mut(&token) {
                    Some(h) => h,
                    None => {
                        tracing::debug!(token = token.0, "orphaned event");
                        continue;
                    }
                };

                // Otherwise this is a waker we need to wake
                if event.is_readable() {
                    tracing::trace!(token = ?token, interest = ?InterestType::Readable, "host epoll");
                    handler.push_interest(InterestType::Readable);
                }
                if event.is_writable() {
                    tracing::trace!(token = ?token, interest = ?InterestType::Writable, "host epoll");
                    handler.push_interest(InterestType::Writable);
                }
                if event.is_read_closed() || event.is_write_closed() {
                    tracing::trace!(token = ?token, interest = ?InterestType::Closed, "host epoll");
                    handler.push_interest(InterestType::Closed);
                }
                if event.is_error() {
                    tracing::trace!(token = ?token, interest = ?InterestType::Error, "host epoll");
                    handler.push_interest(InterestType::Error);
                }
            }
        }
    }
}
