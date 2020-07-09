use crate::{
    error::ExportError,
    export::{Export, Exportable},
    import::LikeNamespace,
    module::{ExportIndex, Module},
    new,
    structures::TypedIndex,
    typed_func::Func,
    types::Value,
    vm,
};
use std::{
    cell::{Ref, RefCell, RefMut},
    error::Error,
    rc::Rc,
};

pub use crate::typed_func::DynamicFunc as DynFunc;

#[derive(Debug)]
pub(crate) struct PreInstance {
    pub(crate) vmctx: Rc<RefCell<vm::Ctx>>,
}

impl PreInstance {
    pub(crate) fn new() -> Self {
        Self {
            vmctx: Rc::new(RefCell::new(unsafe { vm::Ctx::new_uninit() })),
        }
    }

    pub(crate) fn vmctx(&self) -> Rc<RefCell<vm::Ctx>> {
        self.vmctx.clone()
    }

    pub(crate) fn vmctx_ptr(&self) -> *mut vm::Ctx {
        self.vmctx.as_ptr()
    }
}

/// An instantiated WebAssembly module.
///
/// An `Instance` represents a WebAssembly module that
/// has been instantiated with an [`ImportObject`] and is
/// ready to be called.
///
/// [`ImportObject`]: struct.ImportObject.html
pub struct Instance {
    pre_instance: Box<PreInstance>,
    /// The exports of this instance.
    pub exports: Exports,
    pub(crate) new_instance: new::wasmer::Instance,
}

impl Instance {
    pub(crate) fn new(pre_instance: Box<PreInstance>, new_instance: new::wasmer::Instance) -> Self {
        // Initialize the `vm::Ctx`
        {
            let mut vmctx = pre_instance.vmctx.borrow_mut();

            vmctx.module_info = new_instance.module().info() as *const _;
        }

        Self {
            pre_instance,
            exports: new_instance.exports.clone().into(),
            new_instance,
        }
    }

    /// Through generic magic and the awe-inspiring power of traits, we bring you...
    ///
    /// # "Func"
    ///
    /// A [`Func`] allows you to call functions exported from wasm with
    /// near zero overhead.
    ///
    /// [`Func`]: struct.Func.html
    ///
    /// # Usage
    ///
    /// ```
    /// # #![allow(deprecated)]
    /// # use wasmer_runtime_core::{Func, Instance, error::ExportError};
    /// # fn typed_func(instance: Instance) -> Result<(), ExportError> {
    /// let func: Func<(i32, i32)> = instance.func("foo")?;
    ///
    /// func.call(42, 43);
    /// # Ok(())
    /// # }
    /// ```
    #[deprecated(
        since = "0.17.0",
        note = "Please use `instance.exports.get(name)` instead"
    )]
    pub fn func<Args, Rets>(&self, name: &str) -> Result<Func<Args, Rets>, ExportError>
    where
        Args: new::wasmer::WasmTypeList + Clone,
        Rets: new::wasmer::WasmTypeList + Clone,
    {
        self.exports.get(name)
    }

    /// This returns the representation of a function that can be called
    /// safely.
    ///
    /// # Usage
    ///
    /// ```
    /// # #![allow(deprecated)]
    /// # use wasmer_runtime_core::{Instance};
    /// # fn call_foo(instance: &mut Instance) -> Result<(), Box<dyn std::error::Error>> {
    /// instance
    ///     .dyn_func("foo")?
    ///     .call(&[])?;
    /// # Ok(())
    /// # }
    /// ```
    #[deprecated(
        since = "0.17.0",
        note = "Please use `instance.exports.get(name)` instead"
    )]
    pub fn dyn_func(&self, name: &str) -> Result<DynFunc, ExportError> {
        self.exports.get(name)
    }

    /// Resolve an exported function by name.
    pub fn resolve_func(&self, name: &str) -> Result<usize, ()> {
        self.new_instance
            .module()
            .info()
            .exports
            .iter()
            .find_map(|(export_name, export_index)| {
                if name == export_name {
                    match export_index {
                        ExportIndex::Function(index) => Some(index.index()),
                        _ => None,
                    }
                } else {
                    None
                }
            })
            .ok_or(())
    }

    /// Call an exported WebAssembly function given the export name.
    /// Pass arguments by wrapping each one in the [`Value`] enum.
    /// The returned values are also each wrapped in a [`Value`].
    ///
    /// [`Value`]: enum.Value.html
    ///
    /// # Note
    ///
    /// This returns `Result<Vec<Value>, _>` in order to support
    /// the future multi-value returns WebAssembly feature.
    ///
    /// # Usage
    ///
    /// Consider using the more explicit [`Exports::get`]` with [`DynFunc::call`]
    /// instead. For example:
    ///
    /// ```
    /// # use wasmer_runtime_core::{types::Value, Instance, DynFunc};
    /// # fn call_foo(instance: &mut Instance) -> Result<(), Box<dyn std::error::Error>> {
    /// // …
    /// let foo: DynFunc = instance.exports.get("foo")?;
    /// let results = foo.call(&[Value::I32(42)])?;
    /// // …
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// Another example with `Instance::call` directly:
    ///
    /// ```
    /// # use wasmer_runtime_core::{types::Value, Instance};
    /// # fn call_foo(instance: &mut Instance) -> Result<(), Box<dyn std::error::Error>> {
    /// // ...
    /// let results = instance.call("foo", &[Value::I32(42)])?;
    /// // ...
    /// # Ok(())
    /// # }
    /// ```
    pub fn call(&self, name: &str, params: &[Value]) -> Result<Vec<Value>, Box<dyn Error>> {
        Ok(self
            .new_instance
            .exports
            .get_function(name)?
            .call(params)?
            .into_vec())
    }

    /// The module used to instantiate this Instance.
    pub fn module(&self) -> Module {
        Module::new(self.new_instance.module().clone())
    }

    /// Returns an immutable reference to the
    /// [`Ctx`] used by this Instance.
    ///
    /// [`Ctx`]: struct.Ctx.html
    pub fn context(&self) -> Ref<vm::Ctx> {
        self.pre_instance.vmctx.borrow()
    }

    /// Returns a mutable reference to the
    /// [`Ctx`] used by this Instance.
    ///
    /// [`Ctx`]: struct.Ctx.html
    pub fn context_mut(&mut self) -> RefMut<vm::Ctx> {
        self.pre_instance.vmctx.borrow_mut()
    }

    /// Returns an iterator over all of the items
    /// exported from this instance.
    pub fn exports(
        &self,
    ) -> new::wasmer::ExportsIterator<impl Iterator<Item = (&String, &Export)>> {
        self.new_instance.exports.iter()
    }
}

impl LikeNamespace for Instance {
    fn get_namespace_export(&self, name: &str) -> Option<new::wasmer_vm::Export> {
        self.exports.new_exports.get_namespace_export(name)
    }

    fn get_namespace_exports(&self) -> Vec<(String, new::wasmer_vm::Export)> {
        self.exports.new_exports.get_namespace_exports()
    }
}

#[derive(Clone)]
pub struct Exports {
    pub(crate) new_exports: new::wasmer::Exports,
}

impl Exports {
    pub(crate) fn new() -> Self {
        Self {
            new_exports: new::wasmer::Exports::new(),
        }
    }

    pub fn get<'a, T>(&'a self, name: &str) -> Result<T, ExportError>
    where
        T: Exportable<'a> + Clone + 'a,
    {
        Ok(self.new_exports.get::<T>(name)?.clone())
    }

    pub fn iter(&self) -> new::wasmer::ExportsIterator<impl Iterator<Item = (&String, &Export)>> {
        self.new_exports.iter()
    }
}

impl LikeNamespace for Exports {
    fn get_namespace_export(&self, name: &str) -> Option<new::wasmer_vm::Export> {
        self.new_exports.get_namespace_export(name)
    }

    fn get_namespace_exports(&self) -> Vec<(String, new::wasmer_vm::Export)> {
        self.new_exports.get_namespace_exports()
    }
}

impl From<new::wasmer::Exports> for Exports {
    fn from(new_exports: new::wasmer::Exports) -> Self {
        Self { new_exports }
    }
}
