use std::{
    io,
    sync::{Arc, Weak},
};

use mio::Token;

use crate::{InterestHandler, Selector};

#[derive(Debug)]
#[must_use = "Leaking token guards will break the IO subsystem"]
pub struct InterestGuard {
    selector: Weak<Selector>,
    pub(crate) token: Option<Token>,
}
impl Drop for InterestGuard {
    fn drop(&mut self) {
        self.drop_internal();
    }
}
impl InterestGuard {
    pub fn new(
        selector: &Arc<Selector>,
        handler: Box<dyn InterestHandler + Send + Sync>,
        source: &mut dyn mio::event::Source,
        interest: mio::Interest,
    ) -> io::Result<InterestGuard> {
        let token = selector.add(handler, source, interest)?;
        Ok(Self {
            selector: Arc::downgrade(selector),
            token: Some(token),
        })
    }

    pub fn unregister(&mut self, source: &mut dyn mio::event::Source) -> io::Result<()> {
        if let Some(selector) = self.selector.upgrade() {
            if let Some(token) = self.token.take() {
                selector.remove(token, Some(source))?;
            }
        }
        Ok(())
    }

    pub fn replace_handler(
        &mut self,
        handler: Box<dyn InterestHandler + Send + Sync>,
    ) -> Result<(), Box<dyn InterestHandler + Send + Sync>> {
        if let Some(selector) = self.selector.upgrade() {
            if let Some(token) = self.token.take() {
                selector.replace(token, handler);
                Ok(())
            } else {
                Err(handler)
            }
        } else {
            Err(handler)
        }
    }

    fn drop_internal(&mut self) {
        if let Some(token) = self.token.take() {
            if let Some(selector) = self.selector.upgrade() {
                selector.remove(token, None).ok();
            }
        }
    }
}
