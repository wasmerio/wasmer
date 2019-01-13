use crate::recovery::call_protected;
use crate::{
    backing::{ImportBacking, LocalBacking},
    export::{Context, Export, ExportIter, FuncPointer, MemoryPointer, TablePointer},
    import::{Imports, Namespace},
    module::{ExportIndex, Module, ModuleInner},
    types::{FuncIndex, FuncSig, MapIndex, Memory, MemoryIndex, Table, TableIndex, Type, Value},
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

pub struct Instance {
    pub(crate) module: Rc<ModuleInner>,
    inner: Box<InstanceInner>,
}

impl Instance {
    pub(crate) fn new(module: Rc<ModuleInner>, imports: &mut Imports) -> Result<Instance, String> {
        // We need the backing and import_backing to create a vm::Ctx, but we need
        // a vm::Ctx to create a backing and an import_backing. The solution is to create an
        // uninitialized vm::Ctx and then initialize it in-place.
        let mut vmctx = unsafe { Box::new(mem::uninitialized()) };

        let import_backing = ImportBacking::new(&module, imports, &mut *vmctx)?;
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

        let mut instance = Instance { module, inner };

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
    pub fn call(&mut self, name: &str, args: &[Value]) -> Result<Option<Value>, String> {
        let export_index = self
            .module
            .exports
            .get(name)
            .ok_or_else(|| format!("there is no export with that name: {}", name))?;

        let func_index = if let ExportIndex::Func(func_index) = export_index {
            *func_index
        } else {
            return Err("that export is not a function".to_string());
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
    ) -> Result<Option<Value>, String> {
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
            return Err("incorrect signature".to_string());
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

        call_protected(|| {
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
        })
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
                        Context::Internal => {
                            Context::External(&*self.vmctx as *const vm::Ctx as *mut vm::Ctx)
                        }
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
                        Context::Internal => {
                            Context::External(&*self.vmctx as *const vm::Ctx as *mut vm::Ctx)
                        }
                        ctx @ Context::External(_) => ctx,
                    },
                    memory,
                }
            }
            ExportIndex::Global(_global_index) => unimplemented!(),
            ExportIndex::Table(table_index) => {
                let (local, ctx, table) = self.get_table_from_index(*table_index);
                Export::Table {
                    local,
                    ctx: match ctx {
                        Context::Internal => {
                            Context::External(&*self.inner.vmctx as *const vm::Ctx as *mut vm::Ctx)
                        }
                        ctx @ Context::External(_) => ctx,
                    },
                    table,
                }
            },
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

        let (func_ptr, ctx) = if module.is_imported_function(func_index) {
            let imported_func = &self.import_backing.functions[func_index.index()];
            (
                imported_func.func as *const _,
                Context::External(imported_func.vmctx),
            )
        } else {
            (
                module
                    .func_resolver
                    .get(&module, func_index)
                    .expect("broken invariant, func resolver not synced with module.exports")
                    .cast()
                    .as_ptr() as *const _,
                Context::Internal,
            )
        };

        let signature = module.sig_registry.lookup_func_sig(sig_index).clone();

        (unsafe { FuncPointer::new(func_ptr) }, ctx, signature)
    }

    fn get_memory_from_index(
        &mut self,
        module: &ModuleInner,
        mem_index: MemoryIndex,
    ) -> (MemoryPointer, Context, Memory) {
        if module.is_imported_memory(mem_index) {
            let &(_, mem) = &module
                .imported_memories
                .get(mem_index)
                .expect("missing imported memory index");
            let vm::ImportedMemory { memory, vmctx } =
                &self.import_backing.memories[mem_index.index()];
            (
                unsafe { MemoryPointer::new(*memory) },
                Context::External(*vmctx),
                *mem,
            )
        } else {
            let vm_mem = &mut self.backing.memories[mem_index.index() as usize];
            (
                unsafe { MemoryPointer::new(&mut vm_mem.into_vm_memory()) },
                Context::Internal,
                *module
                    .memories
                    .get(mem_index)
                    .expect("broken invariant, memories"),
            )
        }
    }

    fn get_table_from_index(&self, table_index: TableIndex) -> (TablePointer, Context, Table) {
        if self.module.is_imported_table(table_index) {
            let &(_, tab) = &self
                .module
                .imported_tables
                .get(table_index)
                .expect("missing imported table index");
            let vm::ImportedTable { table, vmctx } =
                &self.inner.import_backing.tables[table_index.index()];
            (
                unsafe { TablePointer::new(*table) },
                Context::External(*vmctx),
                *tab,
            )
        } else {
            unimplemented!(); // TODO into_vm_tables requires &mut self
//            let vm_table = &self.inner.backing.tables[table_index.index() as usize];
//            (
//                unsafe { TablePointer::new(&mut vm_table.into_vm_table()) },
//                Context::Internal,
//                *self
//                    .module
//                    .tables
//                    .get(table_index)
//                    .expect("broken invariant, tables"),
//            )
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
    pub fn memory_offset_addr(&self, _index: usize, _offset: usize) -> *const usize {
        unimplemented!("TODO replace this emscripten stub")
    }
}
