//! This module contains types for manipulating and accessing Wasm instances.
//!
//! An "instance", or "instantiated module", is a compiled WebAssembly [`Module`] with its
//! corresponding imports (via [`ImportObject`]) that is ready to execute.
use crate::{
    backend::RunnableModule,
    backing::{ImportBacking, LocalBacking},
    error::{CallResult, InvokeError, ResolveError, ResolveResult, Result, RuntimeError},
    export::{Context, Export, ExportIter, Exportable, FuncPointer},
    global::Global,
    import::{ImportObject, LikeNamespace},
    loader::Loader,
    memory::Memory,
    module::{ExportIndex, Module, ModuleInfo, ModuleInner},
    sig_registry::SigRegistry,
    structures::TypedIndex,
    table::Table,
    typed_func::{Func, Wasm, WasmTypeList},
    types::{FuncIndex, FuncSig, GlobalIndex, LocalOrImport, MemoryIndex, TableIndex, Type, Value},
    vm::{self, InternalField},
};
#[cfg(unix)]
use crate::{
    fault::{pop_code_version, push_code_version},
    state::CodeVersion,
};
use smallvec::{smallvec, SmallVec};
use std::{
    borrow::Borrow,
    mem,
    pin::Pin,
    ptr::{self, NonNull},
    sync::{Arc, Mutex},
};

pub(crate) struct InstanceInner {
    #[allow(dead_code)]
    pub(crate) backing: LocalBacking,
    import_backing: ImportBacking,
    pub(crate) vmctx: *mut vm::Ctx,
    /// Used to control whether or not we `pop` a `CodeVersion` when dropping.
    #[allow(dead_code)]
    code_version_pushed: bool,
}

// manually implemented because InstanceInner contains a raw pointer to Ctx
unsafe impl Send for InstanceInner {}

impl Drop for InstanceInner {
    fn drop(&mut self) {
        // Drop the vmctx.
        unsafe { Box::from_raw(self.vmctx) };
        // Prevent memory leak by freeing the code version; needed for error reporting with Singlepass
        #[cfg(unix)]
        {
            if self.code_version_pushed {
                pop_code_version();
            }
        }
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
    /// The exports of this instance.
    pub exports: Exports,
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
            code_version_pushed: false,
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

        let exports = Exports {
            module: module.clone(),
            instance_inner: &*inner as *const InstanceInner,
        };

        // We need to push the code version so that the exception table can be read
        // in the feault handler so that we can report traps correctly
        #[cfg(unix)]
        {
            let push_code_version_logic = || {
                if let Some(msm) = module.runnable_module.get_module_state_map() {
                    push_code_version(CodeVersion {
                        baseline: true,
                        msm,
                        base: module.runnable_module.get_code()?.as_ptr() as usize,
                        // convert from a `String` to a static string;
                        // can't use `Backend` directly because it's defined in `runtime`.
                        // This is a hack and we need to clean it up.
                        backend: match module.info.backend.as_ref() {
                            "llvm" => "llvm",
                            "cranelift" => "cranelift",
                            "singlepass" => "singlepass",
                            "auto" => "auto",
                            _ => "unknown backend",
                        },
                        runnable_module: module.runnable_module.clone(),
                    });
                    Some(())
                } else {
                    None
                }
            };
            inner.code_version_pushed = push_code_version_logic().is_some();
        }

        let instance = Instance {
            module,
            inner,
            exports,
            import_object: imports.clone(),
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
    #[deprecated(
        since = "0.17.0",
        note = "Please use `instance.exports.get(name)` instead"
    )]
    pub fn func<Args, Rets>(&self, name: &str) -> ResolveResult<Func<Args, Rets, Wasm>>
    where
        Args: WasmTypeList,
        Rets: WasmTypeList,
    {
        self.exports.get(name)
    }

    /// Resolve a function by name.
    pub fn resolve_func(&self, name: &str) -> ResolveResult<usize> {
        resolve_func_index(&*self.module, name).map(|fi| fi.index())
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
    #[deprecated(
        since = "0.17.0",
        note = "Please use `instance.exports.get(name)` instead"
    )]
    pub fn dyn_func(&self, name: &str) -> ResolveResult<DynFunc> {
        self.exports.get(name)
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
    /// Consider using the more explicit [`Exports::get`]` with [`DynFunc::call`]
    /// instead. For example:
    ///
    /// ```
    /// # use wasmer_runtime_core::types::Value;
    /// # use wasmer_runtime_core::error::Result;
    /// # use wasmer_runtime_core::Instance;
    /// # use wasmer_runtime_core::DynFunc;
    /// # fn call_foo(instance: &mut Instance) -> Result<()> {
    /// // ...
    /// let foo: DynFunc = instance.exports.get("foo")?;
    /// let results = foo.call(&[Value::I32(42)])?;
    /// // ...
    /// # Ok(())
    /// # }
    /// ```
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
        let func: DynFunc = self.exports.get(name)?;
        func.call(params)
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

/// Private function used to find the [`FuncIndex`] of a given export.
fn resolve_func_index(module: &ModuleInner, name: &str) -> ResolveResult<FuncIndex> {
    let export_index =
        module
            .info
            .exports
            .get(name)
            .ok_or_else(|| ResolveError::ExportNotFound {
                name: name.to_string(),
            })?;

    if let ExportIndex::Func(func_index) = export_index {
        Ok(*func_index)
    } else {
        Err(ResolveError::ExportWrongType {
            name: name.to_string(),
        }
        .into())
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

    let run_wasm = |result_space: *mut u64| -> CallResult<()> {
        let mut error_out = None;

        let success = unsafe {
            invoke(
                trampoline,
                ctx_ptr,
                func_ptr,
                raw_args.as_ptr(),
                result_space,
                &mut error_out,
                invoke_env,
            )
        };

        if success {
            Ok(())
        } else {
            let error: RuntimeError = error_out.map_or_else(
                || RuntimeError::InvokeError(InvokeError::FailedWithNoError),
                Into::into,
            );
            Err(error.into())
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
            run_wasm(ptr::null_mut())?;
            Ok(())
        }
        &[Type::V128] => {
            let mut result = [0u64; 2];

            run_wasm(result.as_mut_ptr())?;

            let mut bytes = [0u8; 16];
            let lo = result[0].to_le_bytes();
            let hi = result[1].to_le_bytes();
            bytes[..8].clone_from_slice(&lo);
            bytes[8..16].clone_from_slice(&hi);
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

impl<'a> Exportable<'a> for Memory {
    fn get_self(exports: &'a Exports, name: &str) -> ResolveResult<Self> {
        let (inst_inner, module) = exports.get_inner();
        let export_index =
            module
                .info
                .exports
                .get(name)
                .ok_or_else(|| ResolveError::ExportNotFound {
                    name: name.to_string(),
                })?;
        if let ExportIndex::Memory(idx) = export_index {
            Ok(inst_inner.get_memory_from_index(module, *idx))
        } else {
            Err(ResolveError::ExportWrongType {
                name: name.to_string(),
            })
        }
    }
}

impl<'a> Exportable<'a> for Table {
    fn get_self(exports: &'a Exports, name: &str) -> ResolveResult<Self> {
        let (inst_inner, module) = exports.get_inner();
        let export_index =
            module
                .info
                .exports
                .get(name)
                .ok_or_else(|| ResolveError::ExportNotFound {
                    name: name.to_string(),
                })?;
        if let ExportIndex::Table(idx) = export_index {
            Ok(inst_inner.get_table_from_index(module, *idx))
        } else {
            Err(ResolveError::ExportWrongType {
                name: name.to_string(),
            })
        }
    }
}

impl<'a> Exportable<'a> for Global {
    fn get_self(exports: &'a Exports, name: &str) -> ResolveResult<Self> {
        let (inst_inner, module) = exports.get_inner();
        let export_index =
            module
                .info
                .exports
                .get(name)
                .ok_or_else(|| ResolveError::ExportNotFound {
                    name: name.to_string(),
                })?;
        if let ExportIndex::Global(idx) = export_index {
            Ok(inst_inner.get_global_from_index(module, *idx))
        } else {
            Err(ResolveError::ExportWrongType {
                name: name.to_string(),
            })
        }
    }
}

impl<'a> Exportable<'a> for DynFunc<'a> {
    fn get_self(exports: &'a Exports, name: &str) -> ResolveResult<Self> {
        let (inst_inner, module) = exports.get_inner();
        let func_index = resolve_func_index(module, name)?;

        let sig_index = *module
            .info
            .func_assoc
            .get(func_index)
            .expect("broken invariant, incorrect func index");
        let signature = SigRegistry.lookup_signature_ref(&module.info.signatures[sig_index]);

        Ok(DynFunc {
            signature,
            module: &module,
            instance_inner: &inst_inner,
            func_index: func_index,
        })
    }
}

impl<'a, Args: WasmTypeList, Rets: WasmTypeList> Exportable<'a> for Func<'a, Args, Rets, Wasm> {
    fn get_self(exports: &'a Exports, name: &str) -> ResolveResult<Self> {
        let (inst_inner, module) = exports.get_inner();

        let func_index = resolve_func_index(module, name)?;

        let sig_index = *module
            .info
            .func_assoc
            .get(func_index)
            .expect("broken invariant, incorrect func index");
        let signature = SigRegistry.lookup_signature_ref(&module.info.signatures[sig_index]);

        if signature.params() != Args::types() || signature.returns() != Rets::types() {
            Err(ResolveError::Signature {
                expected: (*signature).clone(),
                found: Args::types().to_vec(),
            })?;
        }

        let ctx = match func_index.local_or_import(&module.info) {
            LocalOrImport::Local(_) => inst_inner.vmctx,
            LocalOrImport::Import(imported_func_index) => unsafe {
                inst_inner.import_backing.vm_functions[imported_func_index]
                    .func_ctx
                    .as_ref()
            }
            .vmctx
            .as_ptr(),
        };

        let func_wasm_inner = module
            .runnable_module
            .get_trampoline(&module.info, sig_index)
            .unwrap();

        let (func_ptr, func_env) = match func_index.local_or_import(&module.info) {
            LocalOrImport::Local(local_func_index) => (
                module
                    .runnable_module
                    .get_func(&module.info, local_func_index)
                    .unwrap(),
                None,
            ),
            LocalOrImport::Import(import_func_index) => {
                let imported_func = &inst_inner.import_backing.vm_functions[import_func_index];

                (
                    NonNull::new(imported_func.func as *mut _).unwrap(),
                    unsafe { imported_func.func_ctx.as_ref() }.func_env,
                )
            }
        };

        let typed_func: Func<Args, Rets, Wasm> =
            unsafe { Func::from_raw_parts(func_wasm_inner, func_ptr, func_env, ctx) };

        Ok(typed_func)
    }
}

/// `Exports` is used to get exports like [`Func`]s, [`DynFunc`]s, [`Memory`]s,
/// [`Global`]s, and [`Table`]s from an [`Instance`].
///
/// Use `Instance.exports` to get an `Exports` from an [`Instance`].
pub struct Exports {
    // We want to avoid the borrow checker here.
    // This is safe because
    // 1. `Exports` can't be constructed, its fields inspected (directly or via methods),
    //    or copied outside of this module/in Instance, so it can't safely outlive `Instance`.
    // 2. `InstanceInner` is `Pin<Box<>>`, thus we know that it will not move.
    instance_inner: *const InstanceInner,
    module: Arc<ModuleInner>,
}

// this is safe because the lifetime of `Exports` is tied to `Instance` and
// `*const InstanceInner` comes from a `Pin<Box<InstanceInner>>`
unsafe impl Send for Exports {}

impl Exports {
    /// Get an export.
    ///
    /// ```
    /// # use wasmer_runtime_core::{DynFunc, Func, Instance};
    /// # use wasmer_runtime_core::global::Global;
    /// # use wasmer_runtime_core::types::Value;
    /// # use wasmer_runtime_core::error::ResolveResult;
    /// # fn example_fn(instance: &Instance) -> ResolveResult<()> {
    /// // We can get a function as a static `Func`
    /// let func: Func<i32, i32> = instance.exports.get("my_func")?;
    /// let _result = func.call(42);
    ///
    /// // Or we can get it as a dynamic `DynFunc`
    /// let dyn_func: DynFunc = instance.exports.get("my_func")?;
    /// let _result= dyn_func.call(&[Value::I32(42)]);
    ///
    /// // We can also get other exports like `Global`s, `Memory`s, and `Table`s
    /// let _counter: Global = instance.exports.get("counter")?;
    ///
    /// # Ok(())
    /// # }
    /// ```
    pub fn get<'a, T: Exportable<'a>>(&'a self, name: &str) -> ResolveResult<T> {
        T::get_self(self, name)
    }

    /// This method must remain private for `Exports` to be sound.
    fn get_inner(&self) -> (&InstanceInner, &ModuleInner) {
        let inst_inner = unsafe { &*self.instance_inner };
        let module = self.module.borrow();
        (inst_inner, module)
    }

    /// Iterate the exports.
    ///
    /// ```
    /// # use wasmer_runtime_core::instance::Instance;
    /// # fn iterate_exports_example(instance: &Instance) {
    /// for (export_name, export_value) in instance.exports.into_iter() {
    ///    println!("Found export `{}` with value `{:?}`", export_name, export_value);
    /// }
    /// # }
    /// ```
    pub fn into_iter(&self) -> ExportIter {
        let (inst_inner, module) = self.get_inner();
        ExportIter::new(&module, &inst_inner)
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
