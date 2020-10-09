use super::super::executor::Executor;
use std::any::Any;

pub struct DummyExecutor;

impl Executor for DummyExecutor {
    fn as_any(&self) -> &dyn Any {
        self
    }
}
