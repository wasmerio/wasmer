use crate::errors::InstantiationError;
use crate::exports::Exports;
use crate::module::Module;
use wasmer_vm::{StoreHandle, VMInstance};

use crate::imports::Imports;
use crate::store::AsStoreMut;
use crate::sys::externals::Extern;

/// A WebAssembly Instance is a stateful, executable
/// instance of a WebAssembly [`Module`].
///
/// Instance objects contain all the exported WebAssembly
/// functions, memories, tables and globals that allow
/// interacting with WebAssembly.
///
/// Spec: <https://webassembly.github.io/spec/core/exec/runtime.html#module-instances>
#[derive(Clone)]
pub struct Instance {
    _handle: StoreHandle<VMInstance>,
}

impl From<wasmer_compiler::InstantiationError> for InstantiationError {
    fn from(other: wasmer_compiler::InstantiationError) -> Self {
        match other {
            wasmer_compiler::InstantiationError::Link(e) => Self::Link(e),
            wasmer_compiler::InstantiationError::Start(e) => Self::Start(e),
            wasmer_compiler::InstantiationError::CpuFeature(e) => Self::CpuFeature(e),
        }
    }
}

impl Instance {
    pub(crate) fn new(
        store: &mut impl AsStoreMut,
        module: &Module,
        imports: &Imports,
    ) -> Result<(Self, Exports), InstantiationError> {
        let externs = imports
            .imports_for_module(module)
            .map_err(InstantiationError::Link)?;
        let mut handle = module.0.instantiate(store, &externs)?;
        let exports = Self::get_exports(store, module, &mut handle);

        let instance = Self {
            _handle: StoreHandle::new(store.objects_mut(), handle),
        };

        Ok((instance, exports))
    }

    pub(crate) fn new_by_index(
        store: &mut impl AsStoreMut,
        module: &Module,
        externs: &[Extern],
    ) -> Result<(Self, Exports), InstantiationError> {
        let externs = externs.to_vec();
        let mut handle = module.0.instantiate(store, &externs)?;
        let exports = Self::get_exports(store, module, &mut handle);
        let instance = Self {
            _handle: StoreHandle::new(store.objects_mut(), handle),
        };

        Ok((instance, exports))
    }

    fn get_exports(
        store: &mut impl AsStoreMut,
        module: &Module,
        handle: &mut VMInstance,
    ) -> Exports {
        module
            .exports()
            .map(|export| {
                let name = export.name().to_string();
                let export = handle.lookup(&name).expect("export");
                let extern_ = Extern::from_vm_extern(store, export);
                (name, extern_)
            })
            .collect::<Exports>()
    }
}
