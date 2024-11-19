pub(crate) mod obj;


pub(crate) struct Store {
    pub(crate) engine: Engine,
}

impl Store {
    pub(crate) fn new(engine: Engine) -> Self {
        Self { engine }
    }

    pub(crate) fn engine(&self) -> &Engine {
        &self.engine
    }
}

impl AsEngineRef for Store {
    fn as_engine_ref(&self) -> EngineRef<'_> {
        EngineRef::new(&self.engine)
    }
}
