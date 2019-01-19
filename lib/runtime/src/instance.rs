use crate::recovery::call_protected;
use crate::{
    backing::{ImportBacking, LocalBacking},
    error::{CallError, CallResult, Result},
    export::{
        Context, Export, ExportIter, FuncPointer, GlobalPointer, MemoryPointer, TablePointer,
    },
    import::{Imports, Namespace},
    module::{ExportIndex, Module, ModuleInner},
    types::{
        FuncIndex, FuncSig, GlobalDesc, GlobalIndex, LocalOrImport, Memory, MemoryIndex, Table,
        TableIndex, Type, Value,
    },
    vm,
};
use libffi::high::{arg as libffi_arg, call as libffi_call, CodePtr};
use std::rc::Rc;
use std::{iter, mem};

pub(crate) struct InstanceInner {
    #[allow(dead_code)]
    pub(crate) backing: LocalBacking,
    import_backing: ImportBacking,
    vmctx: Box<vm::Ctx>,
}

/// A WebAssembly instance
pub struct Instance {
    pub module: Rc<ModuleInner>,
    inner: Box<InstanceInner>,
    #[allow(dead_code)]
    imports: Box<Imports>,
}

impl Instance {
    pub(crate) fn new(module: Rc<ModuleInner>, mut imports: Box<Imports>) -> Result<Instance> {
        // We need the backing and import_backing to create a vm::Ctx, but we need
        // a vm::Ctx to create a backing and an import_backing. The solution is to create an
        // uninitialized vm::Ctx and then initialize it in-place.
        let mut vmctx = unsafe { Box::new(mem::uninitialized()) };

        let import_backing = ImportBacking::new(&module, &mut imports, &mut *vmctx)?;
        let backing = LocalBacking::new(&module, &import_backing, &mut *vmctx);

        // When Pin is stablized, this will use `Box::pinned` instead of `Box::new`.
        let mut inner = Box::new(InstanceInner {
            backing,
            import_backing,
            vmctx,
        });

        // Initialize the vm::Ctx in-place after the backing
        // has been boxed.
        *inner.vmctx = unsafe { vm::Ctx::new(&mut inner.backing, &mut inner.import_backing) };

        let mut instance = Instance {
            module,
            inner,
            imports,
        };

        if let Some(start_index) = instance.module.start_func {
            instance.call_with_index(start_index, &[])?;
        }

        Ok(instance)
    }

    /// Call an exported webassembly function given the export name.
    /// Pass arguments by wrapping each one in the `Value` enum.
    /// The returned value is also returned in a `Value`.
    ///
    /// This will eventually return `Result<Option<Vec<Value>>, String>` in
    /// order to support multi-value returns.
    pub fn call(&mut self, name: &str, args: &[Value]) -> CallResult<Option<Value>> {
        let export_index =
            self.module
                .exports
                .get(name)
                .ok_or_else(|| CallError::NoSuchExport {
                    name: name.to_string(),
                })?;

        let func_index = if let ExportIndex::Func(func_index) = export_index {
            *func_index
        } else {
            return Err(CallError::ExportNotFunc {
                name: name.to_string(),
            }
            .into());
        };

        self.call_with_index(func_index, args)
    }

    pub fn exports(&mut self) -> ExportIter {
        ExportIter::new(&self.module, &mut self.inner)
    }

    pub fn module(&self) -> Module {
        Module::new(Rc::clone(&self.module))
    }
}

impl Instance {
    fn call_with_index(
        &mut self,
        func_index: FuncIndex,
        args: &[Value],
    ) -> CallResult<Option<Value>> {
        let (func_ref, ctx, signature) = self.inner.get_func_from_index(&self.module, func_index);

        let func_ptr = CodePtr::from_ptr(func_ref.inner() as _);
        let vmctx_ptr = match ctx {
            Context::External(vmctx) => vmctx,
            Context::Internal => &mut *self.inner.vmctx,
        };

        assert!(
            signature.returns.len() <= 1,
            "multi-value returns not yet supported"
        );

        if !signature.check_sig(args) {
            Err(CallError::Signature {
                expected: signature.clone(),
                found: args.iter().map(|val| val.ty()).collect(),
            })?
        }

        let libffi_args: Vec<_> = args
            .iter()
            .map(|val| match val {
                Value::I32(ref x) => libffi_arg(x),
                Value::I64(ref x) => libffi_arg(x),
                Value::F32(ref x) => libffi_arg(x),
                Value::F64(ref x) => libffi_arg(x),
            })
            .chain(iter::once(libffi_arg(&vmctx_ptr)))
            .collect();

        Ok(call_protected(|| {
            signature
                .returns
                .first()
                .map(|ty| match ty {
                    Type::I32 => Value::I32(unsafe { libffi_call(func_ptr, &libffi_args) }),
                    Type::I64 => Value::I64(unsafe { libffi_call(func_ptr, &libffi_args) }),
                    Type::F32 => Value::F32(unsafe { libffi_call(func_ptr, &libffi_args) }),
                    Type::F64 => Value::F64(unsafe { libffi_call(func_ptr, &libffi_args) }),
                })
                .or_else(|| {
                    // call with no returns
                    unsafe {
                        libffi_call::<()>(func_ptr, &libffi_args);
                    }
                    None
                })
        })?)
    }
}

impl InstanceInner {
    pub(crate) fn get_export_from_index(
        &mut self,
        module: &ModuleInner,
        export_index: &ExportIndex,
    ) -> Export {
        match export_index {
            ExportIndex::Func(func_index) => {
                let (func, ctx, signature) = self.get_func_from_index(module, *func_index);

                Export::Function {
                    func,
                    ctx: match ctx {
                        Context::Internal => Context::External(&mut *self.vmctx),
                        ctx @ Context::External(_) => ctx,
                    },
                    signature,
                }
            }
            ExportIndex::Memory(memory_index) => {
                let (local, ctx, memory) = self.get_memory_from_index(module, *memory_index);
                Export::Memory {
                    local,
                    ctx: match ctx {
                        Context::Internal => Context::External(&mut *self.vmctx),
                        ctx @ Context::External(_) => ctx,
                    },
                    memory,
                }
            }
            ExportIndex::Global(global_index) => {
                let (local, global) = self.get_global_from_index(module, *global_index);
                Export::Global { local, global }
            }
            ExportIndex::Table(table_index) => {
                let (local, ctx, table) = self.get_table_from_index(module, *table_index);
                Export::Table {
                    local,
                    ctx: match ctx {
                        Context::Internal => Context::External(&mut *self.vmctx),
                        ctx @ Context::External(_) => ctx,
                    },
                    table,
                }
            }
        }
    }

    fn get_func_from_index(
        &mut self,
        module: &ModuleInner,
        func_index: FuncIndex,
    ) -> (FuncPointer, Context, FuncSig) {
        let sig_index = *module
            .func_assoc
            .get(func_index)
            .expect("broken invariant, incorrect func index");

        let (func_ptr, ctx) = match func_index.local_or_import(module) {
            LocalOrImport::Local(local_func_index) => (
                module
                    .func_resolver
                    .get(&module, local_func_index)
                    .expect("broken invariant, func resolver not synced with module.exports")
                    .cast()
                    .as_ptr() as *const _,
                Context::Internal,
            ),
            LocalOrImport::Import(imported_func_index) => {
                let imported_func = &self.import_backing.functions[imported_func_index];
                (
                    imported_func.func as *const _,
                    Context::External(imported_func.vmctx),
                )
            }
        };

        let signature = module.sig_registry.lookup_func_sig(sig_index).clone();

        (unsafe { FuncPointer::new(func_ptr) }, ctx, signature)
    }

    fn get_memory_from_index(
        &mut self,
        module: &ModuleInner,
        mem_index: MemoryIndex,
    ) -> (MemoryPointer, Context, Memory) {
        match mem_index.local_or_import(module) {
            LocalOrImport::Local(local_mem_index) => {
                let vm_mem = &mut self.backing.vm_memories[local_mem_index];
                (
                    unsafe { MemoryPointer::new(vm_mem) },
                    Context::Internal,
                    *module
                        .memories
                        .get(local_mem_index)
                        .expect("broken invariant, memories"),
                )
            }
            LocalOrImport::Import(imported_mem_index) => {
                let &(_, mem) = &module
                    .imported_memories
                    .get(imported_mem_index)
                    .expect("missing imported memory index");
                let vm::ImportedMemory { memory, vmctx } =
                    &self.import_backing.memories[imported_mem_index];
                (
                    unsafe { MemoryPointer::new(*memory) },
                    Context::External(*vmctx),
                    *mem,
                )
            }
        }
    }

    fn get_global_from_index(
        &mut self,
        module: &ModuleInner,
        global_index: GlobalIndex,
    ) -> (GlobalPointer, GlobalDesc) {
        match global_index.local_or_import(module) {
            LocalOrImport::Local(local_global_index) => {
                let vm_global = &mut self.backing.vm_globals[local_global_index];
                (
                    unsafe { GlobalPointer::new(vm_global) },
                    module
                        .globals
                        .get(local_global_index)
                        .expect("broken invariant, globals")
                        .desc,
                )
            }
            LocalOrImport::Import(imported_global_index) => {
                let &(_, imported_global_desc) = &module
                    .imported_globals
                    .get(imported_global_index)
                    .expect("missing imported global index");
                let vm::ImportedGlobal { global } =
                    &self.import_backing.globals[imported_global_index];
                (
                    unsafe { GlobalPointer::new(*global) },
                    *imported_global_desc,
                )
            }
        }
    }

    fn get_table_from_index(
        &mut self,
        module: &ModuleInner,
        table_index: TableIndex,
    ) -> (TablePointer, Context, Table) {
        match table_index.local_or_import(module) {
            LocalOrImport::Local(local_table_index) => {
                let vm_table = &mut self.backing.vm_tables[local_table_index];
                (
                    unsafe { TablePointer::new(vm_table) },
                    Context::Internal,
                    *module
                        .tables
                        .get(local_table_index)
                        .expect("broken invariant, tables"),
                )
            }
            LocalOrImport::Import(imported_table_index) => {
                let &(_, tab) = &module
                    .imported_tables
                    .get(imported_table_index)
                    .expect("missing imported table index");
                let vm::ImportedTable { table, vmctx } =
                    &self.import_backing.tables[imported_table_index];
                (
                    unsafe { TablePointer::new(*table) },
                    Context::External(*vmctx),
                    *tab,
                )
            }
        }
    }
}

impl Namespace for Instance {
    fn get_export(&mut self, name: &str) -> Option<Export> {
        let export_index = self.module.exports.get(name)?;

        Some(self.inner.get_export_from_index(&self.module, export_index))
    }
}

// TODO Remove this later, only needed for compilation till emscripten is updated
impl Instance {
    pub fn memory_offset_addr(&self, _index: usize, _offset: usize) -> *const u8 {
        unimplemented!()
    }
}
