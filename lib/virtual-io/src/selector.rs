use std::{
    collections::HashMap,
    io,
    sync::{Arc, Mutex},
};

use mio::{Registry, Token};

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
    fn apply(self, lookup: &mut HashMap<Token, Box<dyn InterestHandler + Send + Sync>>) -> () {
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
                .debug_struct("Replace")
                .field("token", token)
                .field("interest", interest)
                .finish(),
        }
    }
}

#[derive(Debug)]
pub struct Selector {
    token_close: Token,
    /// The core assumption here is that this will always be the innermost lock, so we will never deadlock
    registry: Mutex<Registry>,
    /// See the comment in `run` for the concurrency model
    lookup: Mutex<HashMap<Token, Box<dyn InterestHandler + Send + Sync>>>,
    previous_seed: Mutex<usize>,
    closer: mio::Waker,
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

        let engine = Arc::new(Selector {
            closer: mio::Waker::new(poll.registry(), Token(0)).unwrap(),
            token_close: Token(1),
            previous_seed: Mutex::new(10),
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
        let token = self.generate_token();

        self.queue_modification(SelectorModification::Add { handler, token });

        // CONCURRENCY: This should never result in a deadlock, as inner_registry is only locked for non-blocking operations.
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
        // CONCURRENCY: This should never result in a deadlock, as inner_registry is only locked for non-blocking operations.
        let inner_registry = self.registry.lock().unwrap();
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
    fn generate_token(&self) -> Token {
        // CONCURRENCY: This is safe because inner_seed is only locked here and this function does not do recursion.
        let mut inner_seed = self.previous_seed.lock().unwrap();
        *inner_seed = inner_seed
            .checked_add(1)
            .expect("selector has ran out of token seeds");

        Token(*inner_seed)
    }

    /// Try to process a modification immediately, otherwise queue it up
    fn queue_modification(&self, modification: SelectorModification) {
        // CONCURRENCY: This is safe, because we only lock `queued_modifications` nested which is also safe.
        if let Ok(mut inner_lookup) = self.lookup.try_lock() {
            // We got the inner_lookup lock
            // Process all queued modifications first, to assure they are processed in the correct order
            self.process_queued_modifications(&mut inner_lookup);
            modification.apply(&mut inner_lookup);
        } else {
            // CONCURRENCY: This will never deadlock as queued_modifications is always the innermost lock and we don't call any potentially blocking functions while holding the lock.
            self.queued_modifications.lock().unwrap().push(modification);
        }
    }

    /// Process all queued modifications
    ///
    /// This function must be called with the lookup lock held.
    fn process_queued_modifications(
        &self,
        lookup: &mut HashMap<Token, Box<dyn InterestHandler + Send + Sync>>,
    ) {
        // CONCURRENCY: This will never deadlock as queued_modifications is always the innermost lock and we don't call any potentially blocking functions while holding the lock.
        let queued_modifications = self
            .queued_modifications
            .lock()
            .unwrap()
            .drain(..)
            .collect::<Vec<_>>();
        for modification in queued_modifications {
            modification.apply(lookup);
        }
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

            // CONCURRENCY: Here is a risk for a deadlock because it has a nested lock for `self.queued_modifications`.
            //              See the comment at the nested lock for why this is safe here.
            // CONCURRENCY: Here is a risk for a deadlock because calls the registered handlers which may result in any of the other selector functions being called.
            //              We mitigate that by ensuring ALL OTHER instances of locking `self.inner_lookup` are non-blocking (try_lock) and will just queue up modifications if the lock can not be acquired.
            //              However that still leaves the risk of a recursive call to this function. We prevent that by ???
            let mut inner_lookup = engine.lookup.lock().unwrap();

            for event in events.iter() {
                // If the event is already dropped then ignore it
                let token = event.token();

                // If its the close event then exit
                if token == engine.token_close {
                    return;
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

            // Process any queued up modifications that were caused by the handlers above
            //
            // While we could process the queued modifications already in each loop iteration above, we will do it here because:
            // * If new modifications were added or removed while processing events (in the loop above), they will have been already registered with the source.
            //   We can not delay that as, we only have a reference to the source when adding/removing, so we can not store it for later processing here.
            // * However, as all events have already been collected _before_ entering the loop above, it does not matter if we change the set of registered events in there.
            // * However if we were to switch out the handlers above, we would change the handler for already collected events.
            // * By delaying the processing of the queued handler modifications until here, we ensure that all events will be processed with the handler that was active when the event was collected.
            //
            // tldr; I think this is the right place to process the queued modifications.
            engine.process_queued_modifications(&mut inner_lookup);
        }
    }
}
