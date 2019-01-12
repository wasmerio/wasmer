use crate::recovery::call_protected;
use crate::{
    backing::{ImportBacking, LocalBacking},
    export::{Context, Export},
    import::{ImportResolver, Namespace},
    module::{ExportIndex, Module},
    types::{FuncIndex, FuncSig, MapIndex, Memory, MemoryIndex, Type, Value},
    vm,
};
use hashbrown::hash_map;
use libffi::high::{arg as libffi_arg, call as libffi_call, CodePtr};
use std::rc::Rc;
use std::{iter, mem};

pub struct Instance {
    pub module: Module,
    #[allow(dead_code)]
    pub(crate) backing: LocalBacking,
    #[allow(dead_code)]
    imports: Rc<dyn ImportResolver>,
    import_backing: ImportBacking,
    vmctx: Box<vm::Ctx>,
}

impl Instance {
    pub(crate) fn new(
        module: Module,
        imports: Rc<dyn ImportResolver>,
    ) -> Result<Box<Instance>, String> {
        // We need the backing and import_backing to create a vm::Ctx, but we need
        // a vm::Ctx to create a backing and an import_backing. The solution is to create an
        // uninitialized vm::Ctx and then initialize it in-place.
        let mut vmctx = unsafe { Box::new(mem::uninitialized()) };

        let import_backing = ImportBacking::new(&module, &*imports, &mut *vmctx)?;
        let backing = LocalBacking::new(&module, &import_backing, &mut *vmctx);

        // When Pin is stablized, this will use `Box::pinned` instead of `Box::new`.
        let mut instance = Box::new(Instance {
            module,
            backing,
            imports,
            import_backing,
            vmctx,
        });

        // Initialize the vm::Ctx in-place after the import_backing
        // has been boxed.
        *instance.vmctx = vm::Ctx::new(&mut instance.backing, &mut instance.import_backing);

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

    fn call_with_index(
        &mut self,
        func_index: FuncIndex,
        args: &[Value],
    ) -> Result<Option<Value>, String> {
        let (func_ref, ctx, signature) = self.get_func_from_index(func_index);

        let func_ptr = CodePtr::from_ptr(func_ref.inner() as _);
        let vmctx_ptr = match ctx {
            Context::External(vmctx) => vmctx,
            Context::Internal => &mut *self.vmctx,
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

    pub fn exports(&self) -> ExportIter {
        ExportIter::new(self)
    }

    fn get_export_from_index(&self, export_index: &ExportIndex) -> Export {
        match export_index {
            ExportIndex::Func(func_index) => {
                let (func, ctx, signature) = self.get_func_from_index(*func_index);

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
                let (local, ctx, memory) = self.get_memory_from_index(*memory_index);
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
            ExportIndex::Table(_table_index) => unimplemented!(),
        }
    }

    fn get_func_from_index(&self, func_index: FuncIndex) -> (FuncRef, Context, FuncSig) {
        let sig_index = *self
            .module
            .func_assoc
            .get(func_index)
            .expect("broken invariant, incorrect func index");

        let (func_ptr, ctx) = if self.module.is_imported_function(func_index) {
            let imported_func = &self.import_backing.functions[func_index.index()];
            (
                imported_func.func as *const _,
                Context::External(imported_func.vmctx),
            )
        } else {
            (
                self.module
                    .func_resolver
                    .get(&self.module, func_index)
                    .expect("broken invariant, func resolver not synced with module.exports")
                    .cast()
                    .as_ptr() as *const _,
                Context::Internal,
            )
        };

        let signature = self.module.sig_registry.lookup_func_sig(sig_index).clone();

        (FuncRef(func_ptr), ctx, signature)
    }

    fn get_memory_from_index(
        &self,
        mem_index: MemoryIndex,
    ) -> (*mut vm::LocalMemory, Context, Memory) {
        if self.module.is_imported_memory(mem_index) {
            let &(_, mem) = &self
                .module
                .imported_memories
                .get(mem_index)
                .expect("missing imported memory index");
            let vm::ImportedMemory { memory, vmctx } =
                &self.import_backing.memories[mem_index.index()];
            (*memory, Context::External(*vmctx), *mem)
        } else {
            //           let vm_mem = .memories[mem_index.index() as usize];
            let vm_mem =
                unsafe { &mut (*self.vmctx.local_backing).memories[mem_index.index() as usize] };
            (
                &mut vm_mem.into_vm_memory(),
                Context::Internal,
                *self
                    .module
                    .memories
                    .get(mem_index)
                    .expect("broken invariant, memories"),
            )
        }
    }
}

impl Namespace for Box<Instance> {
    fn get_export(&self, name: &str) -> Option<Export> {
        let export_index = self.module.exports.get(name)?;

        Some(self.get_export_from_index(export_index))
    }
}

#[derive(Debug, Clone)]
pub struct FuncRef(*const vm::Func);

impl FuncRef {
    /// This needs to be unsafe because there is
    /// no way to check whether the passed function
    /// is valid and has the right signature.
    pub unsafe fn new(f: *const vm::Func) -> Self {
        FuncRef(f)
    }

    pub(crate) fn inner(&self) -> *const vm::Func {
        self.0
    }
}

pub struct ExportIter<'a> {
    instance: &'a Instance,
    iter: hash_map::Iter<'a, String, ExportIndex>,
}

impl<'a> ExportIter<'a> {
    fn new(instance: &'a Instance) -> Self {
        Self {
            instance,
            iter: instance.module.exports.iter(),
        }
    }
}

impl<'a> Iterator for ExportIter<'a> {
    type Item = (String, Export);
    fn next(&mut self) -> Option<(String, Export)> {
        let (name, export_index) = self.iter.next()?;
        Some((
            name.clone(),
            self.instance.get_export_from_index(export_index),
        ))
    }
}

// TODO Remove this later, only needed for compilation till emscripten is updated
impl Instance {
    pub fn memory_offset_addr(&self, _index: usize, _offset: usize) -> *const usize {
        unimplemented!("TODO replace this emscripten stub")
    }
}
