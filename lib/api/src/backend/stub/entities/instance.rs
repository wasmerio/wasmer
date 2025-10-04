use crate::{
    exports::Exports,
    store::{AsStoreMut, AsStoreRef},
    Module,
    Extern,
    InstantiationError,
};

/// Minimal instance representation used by the stub backend.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Instance {
    pub(crate) module: Module,
    pub(crate) exports: Exports,
}

impl Instance {
    pub fn new(
        _store: &mut impl AsStoreMut,
        _module: &Module,
        _imports: &[Extern],
    ) -> Result<(Self, Exports), InstantiationError> {
        panic!("stub backend cannot instantiate modules")
    }

    pub fn new_by_index(
        _store: &mut impl AsStoreMut,
        _module: &Module,
        _externs: &[Extern],
    ) -> Result<(Self, Exports), InstantiationError> {
        panic!("stub backend cannot instantiate modules")
    }

    pub fn get_export(&self, _name: &str) -> Option<Extern> {
        panic!("stub backend cannot access exports")
    }

    pub fn exports(&self) -> &Exports {
        &self.exports
    }

    pub fn exports_mut(&mut self) -> &mut Exports {
        &mut self.exports
    }

    pub fn module(&self) -> &Module {
        &self.module
    }

    pub fn is_from_store(&self, _store: &impl AsStoreRef) -> bool {
        panic!("stub backend cannot verify instance origins")
    }
}
