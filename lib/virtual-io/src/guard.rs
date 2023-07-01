use std::{io, sync::Arc};

use mio::Token;

use crate::{InterestHandler, Selector};

pub(crate) struct HandlerWrapper(pub Box<dyn InterestHandler + Send + Sync>);

#[derive(Debug)]
#[must_use = "Leaking token guards will break the IO subsystem"]
pub struct InterestGuard {
    pub(crate) token: Token,
}
impl InterestGuard {
    pub fn new(
        selector: &Arc<Selector>,
        handler: Box<dyn InterestHandler + Send + Sync>,
        source: &mut dyn mio::event::Source,
        interest: mio::Interest,
    ) -> io::Result<InterestGuard> {
        let raw = Box::into_raw(Box::new(HandlerWrapper(handler))) as *const HandlerWrapper;
        let new_token = Token(raw as usize);
        selector.registry.register(source, new_token, interest)?;
        Ok(Self { token: new_token })
    }
    pub fn unregister(
        guard: InterestGuard,
        selector: &Selector,
        source: &mut dyn mio::event::Source,
    ) {
        selector.tx_drop.lock().unwrap().send(guard.token).ok();
        selector.registry.deregister(source).unwrap();
    }
}
