use crate::{
    error::InstantiationError, exports::Exports, imports::Imports, macros::backend::gen_rt_ty,
    module::Module, store::AsStoreMut, Extern,
};

/// A WebAssembly Instance is a stateful, executable
/// instance of a WebAssembly [`Module`].
///
/// Instance objects contain all the exported WebAssembly
/// functions, memories, tables and globals that allow
/// interacting with WebAssembly.
///
/// Spec: <https://webassembly.github.io/spec/core/exec/runtime.html#module-instances>
#[derive(Clone, PartialEq, Eq)]
pub struct Instance {
    pub(crate) _inner: BackendInstance,
    pub(crate) module: Module,
    /// The exports for an instance.
    pub exports: Exports,
}

impl Instance {
    /// Creates a new `Instance` from a WebAssembly [`Module`] and a
    /// set of imports using [`Imports`] or the [`imports!`] macro helper.
    ///
    /// [`imports!`]: crate::imports!
    /// [`Imports!`]: crate::Imports!
    ///
    /// ```
    /// # use wasmer::{imports, Store, Module, Global, Value, Instance};
    /// # use wasmer::FunctionEnv;
    /// # fn main() -> anyhow::Result<()> {
    /// let mut store = Store::default();
    /// let env = FunctionEnv::new(&mut store, ());
    /// let module = Module::new(&store, "(module)")?;
    /// let imports = imports!{
    ///   "host" => {
    ///     "var" => Global::new(&mut store, Value::I32(2))
    ///   }
    /// };
    /// let instance = Instance::new(&mut store, &module, &imports)?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ## Errors
    ///
    /// The function can return [`InstantiationError`]s.
    ///
    /// Those are, as defined by the spec:
    ///  * Link errors that happen when plugging the imports into the instance
    ///  * Runtime errors that happen when running the module `start` function.
    #[allow(clippy::result_large_err)]
    pub fn new(
        store: &mut impl AsStoreMut,
        module: &Module,
        imports: &Imports,
    ) -> Result<Self, InstantiationError> {
        let (_inner, exports) = match &store.as_store_mut().inner.store {
            #[cfg(feature = "sys")]
            crate::BackendStore::Sys(_) => {
                let (i, e) = crate::backend::sys::instance::Instance::new(store, module, imports)?;
                (crate::BackendInstance::Sys(i), e)
            }
            #[cfg(feature = "wamr")]
            crate::BackendStore::Wamr(_) => {
                let (i, e) = crate::backend::wamr::instance::Instance::new(store, module, imports)?;

                (crate::BackendInstance::Wamr(i), e)
            }
            #[cfg(feature = "wasmi")]
            crate::BackendStore::Wasmi(_) => {
                let (i, e) =
                    crate::backend::wasmi::instance::Instance::new(store, module, imports)?;

                (crate::BackendInstance::Wasmi(i), e)
            }
            #[cfg(feature = "v8")]
            crate::BackendStore::V8(_) => {
                let (i, e) = crate::backend::v8::instance::Instance::new(store, module, imports)?;
                (crate::BackendInstance::V8(i), e)
            }
            #[cfg(feature = "js")]
            crate::BackendStore::Js(_) => {
                let (i, e) = crate::backend::js::instance::Instance::new(store, module, imports)?;
                (crate::BackendInstance::Js(i), e)
            }
            #[cfg(feature = "jsc")]
            crate::BackendStore::Jsc(_) => {
                let (i, e) = crate::backend::jsc::instance::Instance::new(store, module, imports)?;
                (crate::BackendInstance::Jsc(i), e)
            }
        };

        Ok(Self {
            _inner,
            module: module.clone(),
            exports,
        })
    }

    /// Creates a new `Instance` from a WebAssembly [`Module`] and a
    /// vector of imports.
    ///
    /// ## Errors
    ///
    /// The function can return [`InstantiationError`]s.
    ///
    /// Those are, as defined by the spec:
    ///  * Link errors that happen when plugging the imports into the instance
    ///  * Runtime errors that happen when running the module `start` function.
    #[allow(clippy::result_large_err)]
    pub fn new_by_index(
        store: &mut impl AsStoreMut,
        module: &Module,
        externs: &[Extern],
    ) -> Result<Self, InstantiationError> {
        let (_inner, exports) = match &store.as_store_mut().inner.store {
            #[cfg(feature = "sys")]
            crate::BackendStore::Sys(_) => {
                let (i, e) =
                    crate::backend::sys::instance::Instance::new_by_index(store, module, externs)?;
                (crate::BackendInstance::Sys(i), e)
            }
            #[cfg(feature = "wamr")]
            crate::BackendStore::Wamr(_) => {
                let (i, e) =
                    crate::backend::wamr::instance::Instance::new_by_index(store, module, externs)?;

                (crate::BackendInstance::Wamr(i), e)
            }
            #[cfg(feature = "wasmi")]
            crate::BackendStore::Wasmi(_) => {
                let (i, e) = crate::backend::wasmi::instance::Instance::new_by_index(
                    store, module, externs,
                )?;

                (crate::BackendInstance::Wasmi(i), e)
            }
            #[cfg(feature = "v8")]
            crate::BackendStore::V8(_) => {
                let (i, e) =
                    crate::backend::v8::instance::Instance::new_by_index(store, module, externs)?;
                (crate::BackendInstance::V8(i), e)
            }
            #[cfg(feature = "js")]
            crate::BackendStore::Js(_) => {
                let (i, e) =
                    crate::backend::js::instance::Instance::new_by_index(store, module, externs)?;
                (crate::BackendInstance::Js(i), e)
            }
            #[cfg(feature = "jsc")]
            crate::BackendStore::Jsc(_) => {
                let (i, e) =
                    crate::backend::jsc::instance::Instance::new_by_index(store, module, externs)?;
                (crate::BackendInstance::Jsc(i), e)
            }
        };

        Ok(Self {
            _inner,
            module: module.clone(),
            exports,
        })
    }

    /// Gets the [`Module`] associated with this instance.
    pub fn module(&self) -> &Module {
        &self.module
    }
}

impl std::fmt::Debug for Instance {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_struct("Instance")
            .field("exports", &self.exports)
            .finish()
    }
}

/// An enumeration of all the possible instances kind supported by the runtimes.
gen_rt_ty!(Instance @derives Clone, PartialEq, Eq);
