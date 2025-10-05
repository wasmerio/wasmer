use crate::{
    backend::stub::panic_stub,
    entities::store::{AsStoreMut, AsStoreRef},
    exports::Exports,
    Extern, InstantiationError, Module,
};

/// Minimal instance representation used by the stub backend.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Instance;

impl Instance {
    pub fn new(
        _store: &mut impl AsStoreMut,
        _module: &Module,
        _imports: &[Extern],
    ) -> Result<(Self, Exports), InstantiationError> {
        panic_stub("cannot instantiate modules")
    }

    pub fn new_by_index(
        _store: &mut impl AsStoreMut,
        _module: &Module,
        _externs: &[Extern],
    ) -> Result<(Self, Exports), InstantiationError> {
        panic_stub("cannot instantiate modules")
    }

    pub fn get_export(&self, _name: &str) -> Option<Extern> {
        panic_stub("cannot access exports")
    }

    pub fn exports(&self) -> &Exports {
        panic_stub("cannot access exports")
    }

    pub fn exports_mut(&mut self) -> &mut Exports {
        panic_stub("cannot access exports")
    }

    pub fn module(&self) -> &Module {
        panic_stub("cannot access instance modules")
    }

    pub fn is_from_store(&self, _store: &impl AsStoreRef) -> bool {
        panic_stub("cannot verify instance origins")
    }
}
