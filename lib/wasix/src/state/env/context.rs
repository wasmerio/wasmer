use futures::{TryFutureExt, channel::oneshot};
use wasmer::RuntimeError;

#[derive(Default, Debug)]
pub struct Context {
    resumer: Option<oneshot::Sender<Result<(), RuntimeError>>>,
}

impl Context {
    // Create a new non-suspended context
    pub fn new() -> Self {
        Default::default()
    }
    // Suspend this context
    //
    // Returns a Future that resolves when the context is resumed
    //
    // Panics if the context is already locked
    pub fn suspend(&mut self) -> impl Future<Output = Result<(), RuntimeError>> + use<> {
        let (sender, receiver) = oneshot::channel();
        if self.resumer.is_some() {
            panic!("Switching from a context that is already switched out");
        }
        self.resumer = Some(sender);
        receiver.unwrap_or_else(|_canceled| {
            // TODO: Handle canceled properly
            // TODO: Think about whether canceled should be handled at all
            todo!("Context was canceled. Cleanup not implemented yet so we just panic");
        })
    }

    // Allow this context to be resumed
    pub fn resume(&mut self, value: Result<(), RuntimeError>) -> () {
        let resumer = self
            .resumer
            .take()
            .expect("Resuming a context that is not switched out");
        resumer.send(value).unwrap();
    }
}

impl Clone for Context {
    fn clone(&self) -> Self {
        if self.resumer.is_some() {
            panic!("Cannot clone a context with a resumer");
        }
        Self { resumer: None }
    }
}
