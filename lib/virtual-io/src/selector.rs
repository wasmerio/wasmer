use mio::{Registry, Token};
use std::{
    collections::HashMap,
    io,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

use crate::{InterestHandler, InterestType};

pub enum SelectorModification {
    Add {
        handler: Box<dyn InterestHandler + Send + Sync>,
        token: Token,
    },
    Remove {
        token: Token,
    },
    Replace {
        token: Token,
        handler: Box<dyn InterestHandler + Send + Sync>,
    },
    PushInterest {
        token: Token,
        interest: InterestType,
    },
}

impl SelectorModification {
    /// Apply the modification to a handler lookup table
    ///
    /// This function must be called with care, as `SelectorModification::PushInterest` may trigger handler code.
    fn apply(self, lookup: &mut HashMap<Token, Box<dyn InterestHandler + Send + Sync>>) {
        match self {
            SelectorModification::Add { token, handler } => {
                lookup.insert(token, handler);
            }
            SelectorModification::Remove { token, .. } => {
                lookup.remove(&token);
            }
            SelectorModification::Replace { token, mut handler } => {
                let last = lookup.remove(&token);

                // If there was a previous handler, copy over its active interests
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

                lookup.insert(token, handler);
            }
            SelectorModification::PushInterest { token, interest } => {
                if let Some(handler) = lookup.get_mut(&token) {
                    handler.push_interest(interest);
                }
            }
        }
    }
}
impl std::fmt::Debug for SelectorModification {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SelectorModification::Add { token, .. } => {
                f.debug_struct("Add").field("token", token).finish()
            }
            SelectorModification::Remove { token, .. } => {
                f.debug_struct("Remove").field("token", token).finish()
            }
            SelectorModification::Replace { token, .. } => {
                f.debug_struct("Replace").field("token", token).finish()
            }
            SelectorModification::PushInterest { token, interest } => f
                .debug_struct("PushInterest")
                .field("token", token)
                .field("interest", interest)
                .finish(),
        }
    }
}

#[derive(Debug)]
pub struct Selector {
    token_close: Token,
    token_wakeup: Token,
    /// The core assumption here is that this will always be the innermost lock, so we will never deadlock
    registry: Mutex<Registry>,
    /// See the comment in `run` for the concurrency model
    lookup: Mutex<HashMap<Token, Box<dyn InterestHandler + Send + Sync>>>,
    next_seed: AtomicUsize,
    closer: mio::Waker,
    // Artifical waker to wake up after PushInterest
    wakeup: mio::Waker,
    /// Queued up modifications that will be processed when we can acquire `inner_lookup` the next time
    /// The core assumption here is that this will always be the innermost lock, so we will never deadlock
    queued_modifications: Mutex<Vec<SelectorModification>>,
}

impl Selector {
    pub fn new() -> Arc<Self> {
        let poll = mio::Poll::new().unwrap();
        let registry = poll
            .registry()
            .try_clone()
            .expect("the selector registry failed to clone");

        let token_close = Token(0);
        let token_wakeup = Token(1);
        let engine = Arc::new(Selector {
            closer: mio::Waker::new(poll.registry(), token_close).unwrap(),
            wakeup: mio::Waker::new(poll.registry(), token_wakeup).unwrap(),
            token_close,
            token_wakeup,
            next_seed: 10.into(),
            lookup: Mutex::new(Default::default()),
            registry: Mutex::new(registry),
            queued_modifications: Mutex::new(Vec::new()),
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
        let token = self.new_token();

        self.queue_modification(SelectorModification::Add { handler, token });

        // CONCURRENCY: This should never result in a deadlock, as long as source.deregister does not call remove or add again.
        let inner_registry = self.registry.lock().unwrap();
        match source.register(&inner_registry, token, interests) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                source.deregister(&inner_registry).ok();
                source.register(&inner_registry, token, interests)?;
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
        self.queue_modification(SelectorModification::Remove { token });
        // CONCURRENCY: This should never result in a deadlock, as long as source.deregister does not call remove or add again.
        let inner_registry = self.registry.try_lock().unwrap();
        if let Some(source) = source {
            inner_registry.deregister(source)?;
        }
        Ok(())
    }

    pub fn push_interest(&self, token: Token, interest: InterestType) {
        self.queue_modification(SelectorModification::PushInterest { token, interest });
    }

    pub fn replace(&self, token: Token, handler: Box<dyn InterestHandler + Send + Sync>) {
        self.queue_modification(SelectorModification::Replace { token, handler });
    }

    /// Generate a new unique token
    #[must_use = "the token must be consumed"]
    fn new_token(&self) -> Token {
        Token(self.next_seed.fetch_add(1, Ordering::Relaxed))
    }

    /// Try to process a modification immediately, otherwise queue it up
    fn queue_modification(&self, modification: SelectorModification) {
        // Replace and PushInterest can cause external code to be called so it is a good idea to process them asap so they don't get delayed too long
        let needs_wakeup = matches!(
            &modification,
            SelectorModification::PushInterest { .. } | SelectorModification::Replace { .. }
        );

        // CONCURRENCY: This will never deadlock as queued_modifications is always the innermost lock and we don't call any potentially blocking functions while holding the lock.
        self.queued_modifications.lock().unwrap().push(modification);

        if needs_wakeup {
            self.wakeup.wake().ok();
        }
    }

    /// Drain the queued modifications queue and return the modifications
    fn take_queued_modifications(&self) -> Vec<SelectorModification> {
        // CONCURRENCY: This will never deadlock as queued_modifications is always the innermost lock and we don't call any potentially blocking functions while holding the lock.
        self.queued_modifications
            .lock()
            .unwrap()
            .drain(..)
            .collect::<Vec<_>>()
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

            // Handler changes that may be queued up between the poll completing and taking the queued modifications can be a problem but we can not eliminate that fully.

            let queued_modifications = engine.take_queued_modifications();

            // Handler changes here are not a problem as they will not effect the current set of events

            // CONCURRENCY: Here is a risk for a deadlock because it has a nested lock for `self.queued_modifications`.
            //              See the comment at the nested lock for why this is safe here.
            // CONCURRENCY: Here is a risk for a deadlock because calls the registered handlers which may result in any of the other selector functions being called.
            //              We mitigate that by ensuring ALL OTHER instances of locking `self.inner_lookup` are non-blocking (try_lock) and will just queue up modifications if the lock can not be acquired.
            //              However that still leaves the risk of a recursive call to this function. We prevent that by ???
            let mut inner_lookup = engine.lookup.lock().unwrap();

            // Process any queued up modifications that were caused by the handlers in the last polling of evetns
            for modification in queued_modifications {
                modification.apply(&mut inner_lookup);
            }

            for event in events.iter() {
                // If the event is already dropped then ignore it
                let token = event.token();

                // If its the close event then exit
                if token == engine.token_close {
                    return;
                }
                if token == engine.token_wakeup {
                    // Just a wake up call, continue to process queued modifications
                    continue;
                }

                // Get the handler
                let handler = match inner_lookup.get_mut(&token) {
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

            // TODO: Race condition: another threade could queue up a modification
            // engine.process_queued_modifications(&mut inner_lookup);
            // Release inner_lookup
            // Release queue
        }
    }
}
