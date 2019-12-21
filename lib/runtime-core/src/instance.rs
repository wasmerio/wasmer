//! The instance module contains the implementation data structures and helper functions used to
//! manipulate and access wasm instances.
use crate::{
    backend::RunnableModule,
    backing::{ImportBacking, LocalBacking},
    error::{CallError, CallResult, ResolveError, ResolveResult, Result, RuntimeError},
    export::{Context, Export, ExportIter, FuncPointer},
    global::Global,
    import::{ImportObject, LikeNamespace},
    loader::Loader,
    memory::Memory,
    module::{ExportIndex, Module, ModuleInfo, ModuleInner},
    sig_registry::SigRegistry,
    structures::TypedIndex,
    table::Table,
    typed_func::{Func, Wasm, WasmTrapInfo, WasmTypeList},
    types::{FuncIndex, FuncSig, GlobalIndex, LocalOrImport, MemoryIndex, TableIndex, Type, Value},
    vm::{self, InternalField},
};
use smallvec::{smallvec, SmallVec};
use std::{
    mem,
    pin::Pin,
    ptr::NonNull,
    sync::{Arc, Mutex},
};

pub(crate) struct InstanceInner {
    #[allow(dead_code)]
    pub(crate) backing: LocalBacking,
    import_backing: ImportBacking,
    pub(crate) vmctx: *mut vm::Ctx,
}

// manually implemented because InstanceInner contains a raw pointer to Ctx
unsafe impl Send for InstanceInner {}

impl Drop for InstanceInner {
    fn drop(&mut self) {
        // Drop the vmctx.
        unsafe { Box::from_raw(self.vmctx) };
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
    /// Reference to the module used to instantiate this instance.
    pub module: Arc<ModuleInner>,
    inner: Pin<Box<InstanceInner>>,
    #[allow(dead_code)]
    import_object: ImportObject,
}

impl Instance {
    pub(crate) fn new(module: Arc<ModuleInner>, imports: &ImportObject) -> Result<Instance> {
        // We need the backing and import_backing to create a vm::Ctx, but we need
        // a vm::Ctx to create a backing and an import_backing. The solution is to create an
        // uninitialized vm::Ctx and then initialize it in-place.
        let mut vmctx: Box<mem::MaybeUninit<vm::Ctx>> =
            Box::new(mem::MaybeUninit::<vm::Ctx>::zeroed());

        let import_backing = ImportBacking::new(&module, &imports, vmctx.as_mut_ptr())?;
        let backing = LocalBacking::new(&module, &import_backing, vmctx.as_mut_ptr())?;

        let mut inner = Box::pin(InstanceInner {
            backing,
            import_backing,
            vmctx: vmctx.as_mut_ptr(),
        });

        // Initialize the vm::Ctx in-place after the backing
        // has been boxed.
        unsafe {
            let backing = &mut *(&mut inner.backing as *mut _);
            let import_backing = &mut *(&mut inner.import_backing as *mut _);
            let real_ctx = match imports.call_state_creator() {
                Some((data, dtor)) => {
                    vm::Ctx::new_with_data(backing, import_backing, &module, data, dtor)
                }
                None => vm::Ctx::new(backing, import_backing, &module),
            };
            vmctx.as_mut_ptr().write(real_ctx);
        };
        Box::leak(vmctx);

        let instance = Instance {
            module,
            inner,
            import_object: imports.clone_ref(),
        };

        if let Some(start_index) = instance.module.info.start_func {
            // We know that the start function takes no arguments and returns no values.
            // Therefore, we can call it without doing any signature checking, etc.

            let func_ptr = match start_index.local_or_import(&instance.module.info) {
                LocalOrImport::Local(local_func_index) => instance
                    .module
                    .runnable_module
                    .get_func(&instance.module.info, local_func_index)
                    .unwrap(),
                LocalOrImport::Import(import_func_index) => NonNull::new(
                    instance.inner.import_backing.vm_functions[import_func_index].func as *mut _,
                )
                .unwrap(),
            };

            let ctx_ptr = match start_index.local_or_import(&instance.module.info) {
                LocalOrImport::Local(_) => instance.inner.vmctx,
                LocalOrImport::Import(imported_func_index) => unsafe {
                    instance.inner.import_backing.vm_functions[imported_func_index]
                        .func_ctx
                        .as_ref()
                }
                .vmctx
                .as_ptr(),
            };

            let sig_index = *instance
                .module
                .info
                .func_assoc
                .get(start_index)
                .expect("broken invariant, incorrect func index");

            let wasm_trampoline = instance
                .module
                .runnable_module
                .get_trampoline(&instance.module.info, sig_index)
                .expect("wasm trampoline");

            let start_func: Func<(), (), Wasm> =
                unsafe { Func::from_raw_parts(wasm_trampoline, func_ptr, None, ctx_ptr) };

            start_func.call()?;
        }

        Ok(instance)
    }

    /// Load an `Instance` using the given loader.
    pub fn load<T: Loader>(&self, loader: T) -> ::std::result::Result<T::Instance, T::Error> {
        loader.load(&**self.module.runnable_module, &self.module.info, unsafe {
            &*self.inner.vmctx
        })
    }

    /// Through generic magic and the awe-inspiring power of traits, we bring you...
    ///
    /// # "Func"
    ///
    /// A [`Func`] allows you to call functions exported from wasm with
    /// near zero overhead.
    ///
    /// [`Func`]: struct.Func.html
    /// # Usage:
    ///
    /// ```
    /// # use wasmer_runtime_core::{Func, Instance, error::ResolveResult};
    /// # fn typed_func(instance: Instance) -> ResolveResult<()> {
    /// let func: Func<(i32, i32)> = instance.func("foo")?;
    ///
    /// func.call(42, 43);
    /// # Ok(())
    /// # }
    /// ```
    pub fn func<Args, Rets>(&self, name: &str) -> ResolveResult<Func<Args, Rets, Wasm>>
    where
        Args: WasmTypeList,
        Rets: WasmTypeList,
    {
        let export_index =
            self.module
                .info
                .exports
                .get(name)
                .ok_or_else(|| ResolveError::ExportNotFound {
                    name: name.to_string(),
                })?;

        if let ExportIndex::Func(func_index) = export_index {
            let sig_index = *self
                .module
                .info
                .func_assoc
                .get(*func_index)
                .expect("broken invariant, incorrect func index");
            let signature =
                SigRegistry.lookup_signature_ref(&self.module.info.signatures[sig_index]);

            if signature.params() != Args::types() || signature.returns() != Rets::types() {
                Err(ResolveError::Signature {
                    expected: (*signature).clone(),
                    found: Args::types().to_vec(),
                })?;
            }

            let ctx = match func_index.local_or_import(&self.module.info) {
                LocalOrImport::Local(_) => self.inner.vmctx,
                LocalOrImport::Import(imported_func_index) => unsafe {
                    self.inner.import_backing.vm_functions[imported_func_index]
                        .func_ctx
                        .as_ref()
                }
                .vmctx
                .as_ptr(),
            };

            let func_wasm_inner = self
                .module
                .runnable_module
                .get_trampoline(&self.module.info, sig_index)
                .unwrap();

            let (func_ptr, func_env) = match func_index.local_or_import(&self.module.info) {
                LocalOrImport::Local(local_func_index) => (
                    self.module
                        .runnable_module
                        .get_func(&self.module.info, local_func_index)
                        .unwrap(),
                    None,
                ),
                LocalOrImport::Import(import_func_index) => {
                    let imported_func = &self.inner.import_backing.vm_functions[import_func_index];

                    (
                        NonNull::new(imported_func.func as *mut _).unwrap(),
                        unsafe { imported_func.func_ctx.as_ref() }.func_env,
                    )
                }
            };

            let typed_func: Func<Args, Rets, Wasm> =
                unsafe { Func::from_raw_parts(func_wasm_inner, func_ptr, func_env, ctx) };

            Ok(typed_func)
        } else {
            Err(ResolveError::ExportWrongType {
                name: name.to_string(),
            }
            .into())
        }
    }

    /// Resolve a function by name.
    pub fn resolve_func(&self, name: &str) -> ResolveResult<usize> {
        let export_index =
            self.module
                .info
                .exports
                .get(name)
                .ok_or_else(|| ResolveError::ExportNotFound {
                    name: name.to_string(),
                })?;

        if let ExportIndex::Func(func_index) = export_index {
            Ok(func_index.index())
        } else {
            Err(ResolveError::ExportWrongType {
                name: name.to_string(),
            }
            .into())
        }
    }

    /// This returns the representation of a function that can be called
    /// safely.
    ///
    /// # Usage:
    /// ```
    /// # use wasmer_runtime_core::Instance;
    /// # use wasmer_runtime_core::error::CallResult;
    /// # fn call_foo(instance: &mut Instance) -> CallResult<()> {
    /// instance
    ///     .dyn_func("foo")?
    ///     .call(&[])?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn dyn_func(&self, name: &str) -> ResolveResult<DynFunc> {
        let export_index =
            self.module
                .info
                .exports
                .get(name)
                .ok_or_else(|| ResolveError::ExportNotFound {
                    name: name.to_string(),
                })?;

        if let ExportIndex::Func(func_index) = export_index {
            let sig_index = *self
                .module
                .info
                .func_assoc
                .get(*func_index)
                .expect("broken invariant, incorrect func index");
            let signature =
                SigRegistry.lookup_signature_ref(&self.module.info.signatures[sig_index]);

            Ok(DynFunc {
                signature,
                module: &self.module,
                instance_inner: &self.inner,
                func_index: *func_index,
            })
        } else {
            Err(ResolveError::ExportWrongType {
                name: name.to_string(),
            }
            .into())
        }
    }

    /// Call an exported WebAssembly function given the export name.
    /// Pass arguments by wrapping each one in the [`Value`] enum.
    /// The returned values are also each wrapped in a [`Value`].
    ///
    /// [`Value`]: enum.Value.html
    ///
    /// # Note:
    /// This returns `CallResult<Vec<Value>>` in order to support
    /// the future multi-value returns WebAssembly feature.
    ///
    /// # Usage:
    /// ```
    /// # use wasmer_runtime_core::types::Value;
    /// # use wasmer_runtime_core::error::Result;
    /// # use wasmer_runtime_core::Instance;
    /// # fn call_foo(instance: &mut Instance) -> Result<()> {
    /// // ...
    /// let results = instance.call("foo", &[Value::I32(42)])?;
    /// // ...
    /// # Ok(())
    /// # }
    /// ```
    pub fn call(&self, name: &str, params: &[Value]) -> CallResult<Vec<Value>> {
        let export_index =
            self.module
                .info
                .exports
                .get(name)
                .ok_or_else(|| ResolveError::ExportNotFound {
                    name: name.to_string(),
                })?;

        let func_index = if let ExportIndex::Func(func_index) = export_index {
            *func_index
        } else {
            return Err(CallError::Resolve(ResolveError::ExportWrongType {
                name: name.to_string(),
            })
            .into());
        };

        let mut results = Vec::new();

        call_func_with_index(
            &self.module.info,
            &**self.module.runnable_module,
            &self.inner.import_backing,
            self.inner.vmctx,
            func_index,
            params,
            &mut results,
        )?;

        Ok(results)
    }

    /// Returns an immutable reference to the
    /// [`Ctx`] used by this Instance.
    ///
    /// [`Ctx`]: struct.Ctx.html
    pub fn context(&self) -> &vm::Ctx {
        unsafe { &*self.inner.vmctx }
    }

    /// Returns a mutable reference to the
    /// [`Ctx`] used by this Instance.
    ///
    /// [`Ctx`]: struct.Ctx.html
    pub fn context_mut(&mut self) -> &mut vm::Ctx {
        unsafe { &mut *self.inner.vmctx }
    }

    /// Returns an iterator over all of the items
    /// exported from this instance.
    pub fn exports(&self) -> ExportIter {
        ExportIter::new(&self.module, &self.inner)
    }

    /// The module used to instantiate this Instance.
    pub fn module(&self) -> Module {
        Module::new(Arc::clone(&self.module))
    }

    /// Get the value of an internal field
    pub fn get_internal(&self, field: &InternalField) -> u64 {
        self.inner.backing.internals.0[field.index()]
    }

    /// Set the value of an internal field.
    pub fn set_internal(&mut self, field: &InternalField, value: u64) {
        self.inner.backing.internals.0[field.index()] = value;
    }
}

impl InstanceInner {
    pub(crate) fn get_export_from_index(
        &self,
        module: &ModuleInner,
        export_index: &ExportIndex,
    ) -> Export {
        match export_index {
            ExportIndex::Func(func_index) => {
                let (func, ctx, signature) = self.get_func_from_index(module, *func_index);

                Export::Function {
                    func,
                    ctx: match ctx {
                        Context::Internal => Context::External(self.vmctx),
                        ctx @ Context::External(_) => ctx,
                        ctx @ Context::ExternalWithEnv(_, _) => ctx,
                    },
                    signature,
                }
            }
            ExportIndex::Memory(memory_index) => {
                let memory = self.get_memory_from_index(module, *memory_index);
                Export::Memory(memory)
            }
            ExportIndex::Global(global_index) => {
                let global = self.get_global_from_index(module, *global_index);
                Export::Global(global)
            }
            ExportIndex::Table(table_index) => {
                let table = self.get_table_from_index(module, *table_index);
                Export::Table(table)
            }
        }
    }

    fn get_func_from_index(
        &self,
        module: &ModuleInner,
        func_index: FuncIndex,
    ) -> (FuncPointer, Context, Arc<FuncSig>) {
        let sig_index = *module
            .info
            .func_assoc
            .get(func_index)
            .expect("broken invariant, incorrect func index");

        let (func_ptr, ctx) = match func_index.local_or_import(&module.info) {
            LocalOrImport::Local(local_func_index) => (
                module
                    .runnable_module
                    .get_func(&module.info, local_func_index)
                    .expect("broken invariant, func resolver not synced with module.exports")
                    .cast()
                    .as_ptr() as *const _,
                Context::Internal,
            ),
            LocalOrImport::Import(imported_func_index) => {
                let imported_func = &self.import_backing.vm_functions[imported_func_index];
                let func_ctx = unsafe { imported_func.func_ctx.as_ref() };

                (
                    imported_func.func as *const _,
                    Context::ExternalWithEnv(func_ctx.vmctx.as_ptr(), func_ctx.func_env),
                )
            }
        };

        let signature = SigRegistry.lookup_signature_ref(&module.info.signatures[sig_index]);

        (unsafe { FuncPointer::new(func_ptr) }, ctx, signature)
    }

    fn get_memory_from_index(&self, module: &ModuleInner, mem_index: MemoryIndex) -> Memory {
        match mem_index.local_or_import(&module.info) {
            LocalOrImport::Local(local_mem_index) => self.backing.memories[local_mem_index].clone(),
            LocalOrImport::Import(imported_mem_index) => {
                self.import_backing.memories[imported_mem_index].clone()
            }
        }
    }

    fn get_global_from_index(&self, module: &ModuleInner, global_index: GlobalIndex) -> Global {
        match global_index.local_or_import(&module.info) {
            LocalOrImport::Local(local_global_index) => {
                self.backing.globals[local_global_index].clone()
            }
            LocalOrImport::Import(import_global_index) => {
                self.import_backing.globals[import_global_index].clone()
            }
        }
    }

    fn get_table_from_index(&self, module: &ModuleInner, table_index: TableIndex) -> Table {
        match table_index.local_or_import(&module.info) {
            LocalOrImport::Local(local_table_index) => {
                self.backing.tables[local_table_index].clone()
            }
            LocalOrImport::Import(imported_table_index) => {
                self.import_backing.tables[imported_table_index].clone()
            }
        }
    }
}

impl LikeNamespace for Instance {
    fn get_export(&self, name: &str) -> Option<Export> {
        let export_index = self.module.info.exports.get(name)?;

        Some(self.inner.get_export_from_index(&self.module, export_index))
    }

    fn get_exports(&self) -> Vec<(String, Export)> {
        unimplemented!("Use the exports method instead");
    }

    fn maybe_insert(&mut self, _name: &str, _export: Export) -> Option<()> {
        None
    }
}

use std::rc::Rc;
impl LikeNamespace for Rc<Instance> {
    fn get_export(&self, name: &str) -> Option<Export> {
        let export_index = self.module.info.exports.get(name)?;

        Some(self.inner.get_export_from_index(&self.module, export_index))
    }

    fn get_exports(&self) -> Vec<(String, Export)> {
        unimplemented!("Use the exports method instead");
    }

    fn maybe_insert(&mut self, _name: &str, _export: Export) -> Option<()> {
        None
    }
}

impl LikeNamespace for Arc<Mutex<Instance>> {
    fn get_export(&self, name: &str) -> Option<Export> {
        let instance = self.lock().unwrap();
        let export_index = instance.module.info.exports.get(name)?;

        Some(
            instance
                .inner
                .get_export_from_index(&instance.module, export_index),
        )
    }

    fn get_exports(&self) -> Vec<(String, Export)> {
        unimplemented!("Use the exports method instead");
    }

    fn maybe_insert(&mut self, _name: &str, _export: Export) -> Option<()> {
        None
    }
}

#[must_use]
fn call_func_with_index(
    info: &ModuleInfo,
    runnable: &dyn RunnableModule,
    import_backing: &ImportBacking,
    local_ctx: *mut vm::Ctx,
    func_index: FuncIndex,
    args: &[Value],
    rets: &mut Vec<Value>,
) -> CallResult<()> {
    let sig_index = *info
        .func_assoc
        .get(func_index)
        .expect("broken invariant, incorrect func index");

    let signature = &info.signatures[sig_index];

    let func_ptr = match func_index.local_or_import(info) {
        LocalOrImport::Local(local_func_index) => {
            runnable.get_func(info, local_func_index).unwrap()
        }
        LocalOrImport::Import(import_func_index) => {
            NonNull::new(import_backing.vm_functions[import_func_index].func as *mut _).unwrap()
        }
    };

    let ctx_ptr = match func_index.local_or_import(info) {
        LocalOrImport::Local(_) => local_ctx,
        LocalOrImport::Import(imported_func_index) => unsafe {
            import_backing.vm_functions[imported_func_index]
                .func_ctx
                .as_ref()
        }
        .vmctx
        .as_ptr(),
    };

    let wasm = runnable
        .get_trampoline(info, sig_index)
        .expect("wasm trampoline");

    call_func_with_index_inner(ctx_ptr, func_ptr, signature, wasm, args, rets)
}

pub(crate) fn call_func_with_index_inner(
    ctx_ptr: *mut vm::Ctx,
    func_ptr: NonNull<vm::Func>,
    signature: &FuncSig,
    wasm: Wasm,
    args: &[Value],
    rets: &mut Vec<Value>,
) -> CallResult<()> {
    rets.clear();

    let num_results = signature.returns().len();
    let num_results = num_results
        + signature
            .returns()
            .iter()
            .filter(|&&ty| ty == Type::V128)
            .count();
    rets.reserve(num_results);

    if !signature.check_param_value_types(args) {
        Err(ResolveError::Signature {
            expected: signature.clone(),
            found: args.iter().map(|val| val.ty()).collect(),
        })?
    }

    let mut raw_args: SmallVec<[u64; 8]> = SmallVec::new();
    for v in args {
        match v {
            Value::I32(i) => {
                raw_args.push(*i as u64);
            }
            Value::I64(i) => {
                raw_args.push(*i as u64);
            }
            Value::F32(f) => {
                raw_args.push(f.to_bits() as u64);
            }
            Value::F64(f) => {
                raw_args.push(f.to_bits() as u64);
            }
            Value::V128(v) => {
                let bytes = v.to_le_bytes();
                let mut lo = [0u8; 8];
                lo.clone_from_slice(&bytes[0..8]);
                raw_args.push(u64::from_le_bytes(lo));
                let mut hi = [0u8; 8];
                hi.clone_from_slice(&bytes[8..16]);
                raw_args.push(u64::from_le_bytes(hi));
            }
        }
    }

    let Wasm {
        trampoline,
        invoke,
        invoke_env,
    } = wasm;

    let run_wasm = |result_space: *mut u64| unsafe {
        let mut trap_info = WasmTrapInfo::Unknown;
        let mut user_error = None;

        let success = invoke(
            trampoline,
            ctx_ptr,
            func_ptr,
            raw_args.as_ptr(),
            result_space,
            &mut trap_info,
            &mut user_error,
            invoke_env,
        );

        if success {
            Ok(())
        } else {
            if let Some(data) = user_error {
                Err(RuntimeError::Error { data })
            } else {
                Err(RuntimeError::Trap {
                    msg: trap_info.to_string().into(),
                })
            }
        }
    };

    let raw_to_value = |raw, ty| match ty {
        Type::I32 => Value::I32(raw as i32),
        Type::I64 => Value::I64(raw as i64),
        Type::F32 => Value::F32(f32::from_bits(raw as u32)),
        Type::F64 => Value::F64(f64::from_bits(raw)),
        Type::V128 => unreachable!("V128 does not map to any single value"),
    };

    match signature.returns() {
        &[] => {
            run_wasm(0 as *mut u64)?;
            Ok(())
        }
        &[Type::V128] => {
            let mut result = [0u64; 2];

            run_wasm(result.as_mut_ptr())?;

            let mut bytes = [0u8; 16];
            let lo = result[0].to_le_bytes();
            let hi = result[1].to_le_bytes();
            for i in 0..8 {
                bytes[i] = lo[i];
                bytes[i + 8] = hi[i];
            }
            rets.push(Value::V128(u128::from_le_bytes(bytes)));
            Ok(())
        }
        &[ty] => {
            let mut result = 0u64;

            run_wasm(&mut result)?;

            rets.push(raw_to_value(result, ty));

            Ok(())
        }
        result_tys @ _ => {
            let mut results: SmallVec<[u64; 8]> = smallvec![0; num_results];

            run_wasm(results.as_mut_ptr())?;

            rets.extend(
                results
                    .iter()
                    .zip(result_tys.iter())
                    .map(|(&raw, &ty)| raw_to_value(raw, ty)),
            );

            Ok(())
        }
    }
}

/// A representation of an exported WebAssembly function.
pub struct DynFunc<'a> {
    pub(crate) signature: Arc<FuncSig>,
    module: &'a ModuleInner,
    pub(crate) instance_inner: &'a InstanceInner,
    func_index: FuncIndex,
}

impl<'a> DynFunc<'a> {
    /// Call an exported WebAssembly function safely.
    ///
    /// Pass arguments by wrapping each one in the [`Value`] enum.
    /// The returned values are also each wrapped in a [`Value`].
    ///
    /// [`Value`]: enum.Value.html
    ///
    /// # Note:
    /// This returns `CallResult<Vec<Value>>` in order to support
    /// the future multi-value returns WebAssembly feature.
    ///
    /// # Usage:
    /// ```
    /// # use wasmer_runtime_core::Instance;
    /// # use wasmer_runtime_core::error::CallResult;
    /// # fn call_foo(instance: &mut Instance) -> CallResult<()> {
    /// instance
    ///     .dyn_func("foo")?
    ///     .call(&[])?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn call(&self, params: &[Value]) -> CallResult<Vec<Value>> {
        let mut results = Vec::new();

        call_func_with_index(
            &self.module.info,
            &**self.module.runnable_module,
            &self.instance_inner.import_backing,
            self.instance_inner.vmctx,
            self.func_index,
            params,
            &mut results,
        )?;

        Ok(results)
    }

    /// Gets the signature of this `Dynfunc`.
    pub fn signature(&self) -> &FuncSig {
        &*self.signature
    }

    /// Gets a const pointer to the function represent by this `DynFunc`.
    pub fn raw(&self) -> *const vm::Func {
        match self.func_index.local_or_import(&self.module.info) {
            LocalOrImport::Local(local_func_index) => self
                .module
                .runnable_module
                .get_func(&self.module.info, local_func_index)
                .unwrap()
                .as_ptr(),
            LocalOrImport::Import(import_func_index) => {
                self.instance_inner.import_backing.vm_functions[import_func_index].func
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn is_send<T: Send>() {}

    #[test]
    fn test_instance_is_send() {
        is_send::<Instance>();
    }
}
