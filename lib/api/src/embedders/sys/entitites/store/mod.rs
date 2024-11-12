use wasmer_compiler::Engine;
use wasmer_vm::{init_traps, TrapHandlerFn};

use crate::StoreLike;

pub(crate) struct Store {
    pub(crate) engine: Engine,

    pub(crate) trap_handler: Option<Box<TrapHandlerFn<'static>>>,
}

impl StoreLike for Store {
    fn new(engine: impl Into<crate::Engine>) -> Self
    where
        Self: Sized,
    {
        Into::into(engine).0.default_store()
    }

    fn engine(&self) -> &crate::Engine {
        todo!()
    }

    fn engine_mut(&mut self) -> &mut crate::Engine {
        todo!()
    }

    fn get_embedder(&self) -> crate::embedders::Embedder {
        todo!()
    }
}
