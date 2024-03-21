use std::sync::Arc;
use std::sync::Mutex;

use derivative::Derivative;

use crate::{InterestHandler, InterestType};

#[derive(Derivative)]
#[derivative(Debug)]
struct ArcInterestHandlerState {
    #[derivative(Debug = "ignore")]
    handler: Box<dyn InterestHandler + Send + Sync>,
}

#[derive(Debug, Clone)]
pub struct ArcInterestHandler {
    state: Arc<Mutex<ArcInterestHandlerState>>,
}

impl ArcInterestHandler {
    pub fn new(handler: Box<dyn InterestHandler + Send + Sync>) -> Self {
        Self {
            state: Arc::new(Mutex::new(ArcInterestHandlerState { handler })),
        }
    }
}

impl InterestHandler for ArcInterestHandler {
    fn push_interest(&mut self, interest: InterestType) {
        let mut state = self.state.lock().unwrap();
        state.handler.push_interest(interest)
    }

    fn pop_interest(&mut self, interest: InterestType) -> bool {
        let mut state = self.state.lock().unwrap();
        state.handler.pop_interest(interest)
    }

    fn has_interest(&self, interest: InterestType) -> bool {
        let state = self.state.lock().unwrap();
        state.handler.has_interest(interest)
    }
}
