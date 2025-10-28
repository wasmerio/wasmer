use mio::{Registry, Token};
use std::{
    collections::HashMap,
    io,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
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
            SelectorModification::Remove { token } => {
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
    /// Set to true when exiting the poll loop
    close_requested: AtomicBool,
    token_wakeup: Token,
    /// The core assumption here is that this will always be the innermost lock, so we will never deadlock
    registry: Mutex<Registry>,
    next_seed: AtomicUsize,
    /// Waker to wake up the selectors own poll loop
    wakeup: mio::Waker,
    /// Queued up modifications that will be processed immediately after we get new events
    ///
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

        let token_wakeup = Token(0);
        let engine = Arc::new(Selector {
            wakeup: mio::Waker::new(poll.registry(), token_wakeup).unwrap(),
            close_requested: false.into(),
            token_wakeup,
            next_seed: 10.into(),
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
        self.close_requested.store(true, Ordering::SeqCst);
        self.wakeup.wake().ok();
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
        if let Some(source) = source {
            let inner_registry = self.registry.lock().unwrap();
            source.deregister(&inner_registry)?;
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
        let mut handler_map: HashMap<Token, Box<dyn InterestHandler + Send + Sync>> =
            HashMap::new();
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
            for modification in queued_modifications {
                modification.apply(&mut handler_map);
            }

            for event in events.iter() {
                // If the event is already dropped then ignore it
                let token = event.token();

                if token == engine.token_wakeup {
                    if engine.close_requested.load(Ordering::SeqCst) {
                        // If exiting was requested, exit the loop
                        return;
                    }
                    // Just a wake up call, continue to process queued modifications
                    continue;
                }

                // Get the handler
                let Some(handler) = handler_map.get_mut(&token) else {
                    tracing::debug!(token = token.0, "orphaned event");
                    continue;
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

// Tests only run on unix because they depend on mio's unix pipe implementation
#[cfg(all(unix, test))]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    #[derive(Debug)]
    struct TestHandler {
        success_sender: mpsc::Sender<()>,
    }
    impl InterestHandler for TestHandler {
        fn push_interest(&mut self, _interest: InterestType) {
            // Send if we received an interest
            self.success_sender.send(()).unwrap();
        }

        fn pop_interest(&mut self, _interest: InterestType) -> bool {
            false
        }

        fn has_interest(&self, interest: InterestType) -> bool {
            interest == InterestType::Readable
        }
    }

    #[derive(Debug)]
    struct DeadlockingHandler {
        selector: Arc<Selector>,
        token: Arc<Mutex<Option<Token>>>,
        success_sender: mpsc::Sender<()>,
    }
    impl InterestHandler for DeadlockingHandler {
        fn push_interest(&mut self, _interest: InterestType) {
            // This would deadlock without a queue
            self.selector
                .remove(self.token.lock().unwrap().unwrap(), None)
                .unwrap();
            self.success_sender.send(()).unwrap();
        }

        fn pop_interest(&mut self, _interest: InterestType) -> bool {
            false
        }

        fn has_interest(&self, interest: InterestType) -> bool {
            interest == InterestType::Readable
        }
    }

    #[test]
    fn test_push_interest() {
        let (mut sender, mut receiver) = mio::unix::pipe::new().unwrap();
        let (success_sender, success_receiver) = std::sync::mpsc::channel();

        let selector = Selector::new();

        let handler = Box::new(TestHandler { success_sender });

        let token = selector
            .add(handler, &mut receiver, mio::Interest::READABLE)
            .unwrap();

        assert!(
            success_receiver.try_recv().is_err(),
            "Received success before sending data. Something is wrong"
        );

        thread::sleep(Duration::from_millis(10));

        // Works once
        sender.write_all(&[1, 2, 3]).unwrap();
        sender.flush().unwrap();
        thread::sleep(Duration::from_millis(10));
        assert!(
            success_receiver.try_recv().is_ok(),
            "Did not receive success signal from handler"
        );
        assert!(
            success_receiver.try_recv().is_err(),
            "Did receive more than once from handler"
        );
        thread::sleep(Duration::from_millis(10));

        // Works multiple times
        sender.write_all(&[1, 2, 3]).unwrap();
        sender.flush().unwrap();
        thread::sleep(Duration::from_millis(10));
        assert!(
            success_receiver.try_recv().is_ok(),
            "Did not receive success signal from handler"
        );
        assert!(
            success_receiver.try_recv().is_err(),
            "Did receive more than once from handler"
        );
        thread::sleep(Duration::from_millis(10));

        // No signal after removing the handler
        selector.remove(token, Some(&mut receiver)).unwrap();
        sender.write_all(&[1, 2, 3]).unwrap();
        sender.flush().unwrap();
        thread::sleep(Duration::from_millis(10));
        assert!(
            success_receiver.try_recv().is_err(),
            "Did receive even though the handler was removed"
        );
        thread::sleep(Duration::from_millis(10));

        selector.shutdown();
    }

    #[test]
    fn test_selector_no_deadlock_when_modifying_the_selector_from_push_interest() {
        let (mut sender, mut receiver) = mio::unix::pipe::new().unwrap();
        let (success_sender, success_receiver) = std::sync::mpsc::channel();

        let selector = Selector::new();

        // The deadlocking handler will try to remove itself from the selector when it receives an interest
        let handler = Box::new(DeadlockingHandler {
            selector: selector.clone(),
            token: Default::default(),
            success_sender,
        });
        let handler_token_arcmutex = handler.token.clone();

        let token = selector
            .add(handler, &mut receiver, mio::Interest::READABLE)
            .unwrap();
        handler_token_arcmutex.lock().unwrap().replace(token);

        sender.write_all(&[1, 2, 3]).unwrap();
        sender.flush().unwrap();

        thread::sleep(Duration::from_millis(100));
        selector.shutdown();
        thread::sleep(Duration::from_millis(100));

        let received_result = success_receiver.try_recv();
        assert!(
            received_result.is_ok(),
            "Did not receive success signal from handler, deadlocked?"
        );
    }
}
