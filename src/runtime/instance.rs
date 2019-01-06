use crate::apis::emscripten::InstanceEnvironment;
use crate::recovery::call_protected;
use crate::runtime::{
    backing::{ImportBacking, LocalBacking},
    memory::LinearMemory,
    module::{Export, Module},
    sig_registry::SigRegistry,
    table::TableBacking,
    types::{FuncIndex, FuncSig, Memory, Table, Type, Value},
    vm,
};
use hashbrown::HashMap;
use libffi::high::{arg as libffi_arg, call as libffi_call, CodePtr};
use std::cell::UnsafeCell;
use std::iter;
use std::sync::Arc;

pub struct Instance {
    pub(in crate::runtime) backing: LocalBacking,
    import_backing: ImportBacking,
    sig_registry: SigRegistry,
    pub module: Arc<Module>,
    pub environments: Vec<InstanceEnvironment>,
}

impl Instance {
    pub fn get_function_pointer(&self, func_index: FuncIndex) -> *const u8 {
        unimplemented!()
        //        get_function_addr(&func_index, &self.import_functions, &self.functions)
    }

    // TODO visibility (in crate::runtime)
    pub fn new(module: Arc<Module>, imports: &dyn ImportResolver) -> Result<Box<Instance>, String> {
        let sig_registry = SigRegistry::new(&*module);

        let import_backing = ImportBacking::new(&*module, imports)?;
        let backing = LocalBacking::new(&*module, &import_backing, &sig_registry);

        let start_func = module.start_func;

        let mut instance = Box::new(Instance {
            backing,
            import_backing,
            module: Arc::clone(&module),
            sig_registry,
            environments: vec![],
        });

        if let Some(start_index) = start_func {
            instance.call_with_index(start_index, &[])?;
        }

        module.after_instantiate(&mut instance);

        Ok(instance)
    }

    /// Call an exported webassembly function given the export name.
    /// Pass arguments by wrapping each one in the `Val` enum.
    /// The returned value is also returned in a `Val`.
    ///
    /// This will eventually return `Result<Option<Vec<Val>>, String>` in
    /// order to support multi-value returns.
    pub fn call(&mut self, name: &str, args: &[Value]) -> Result<Option<Value>, String> {
        let func_index = *self
            .module
            .exports
            .get(name)
            .ok_or_else(|| "there is no export with that name".to_string())
            .and_then(|export| match export {
                Export::Func(func_index) => Ok(func_index),
                _ => Err("that export is not a function".to_string()),
            })?;

        self.call_with_index(func_index, args)
    }

    fn call_with_index(
        &mut self,
        func_index: FuncIndex,
        args: &[Value],
    ) -> Result<Option<Value>, String> {
        // Check the function signature.
        let sig_index = *self
            .module
            .signature_assoc
            .get(func_index)
            .expect("broken invariant, incorrect func index");

        {
            let signature = &self.module.signatures[sig_index];

            assert!(
                signature.returns.len() <= 1,
                "multi-value returns not yet supported"
            );

            if !signature.check_sig(args) {
                return Err("incorrect signature".to_string());
            }
        }

        // the vmctx will be located at the same place on the stack the entire time that this
        // wasm function is running.
        let mut vmctx = vm::Ctx::new(
            &mut self.backing,
            &mut self.import_backing,
            &self.sig_registry,
        );
        let vmctx_ptr = &mut vmctx as *mut vm::Ctx;

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

        let func_ptr = CodePtr::from_ptr(
            self.module
                .func_resolver
                .get(&*self.module, func_index)
                .expect("broken invariant, func resolver not synced with module.exports")
                .cast()
                .as_ptr(),
        );

        call_protected(|| {
            self.module.signatures[sig_index]
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

#[derive(Debug)]
pub enum Import {
    Func(*const vm::Func, FuncSig),
    Table(Arc<TableBacking>, Table),
    Memory(Arc<LinearMemory>, Memory),
    Global(Value),
}

pub struct Imports {
    map: HashMap<String, HashMap<String, Import>>,
}

impl Imports {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn add(&mut self, module: String, name: String, import: Import) {
        self.map
            .entry(module)
            .or_insert(HashMap::new())
            .insert(name, import);
    }

    pub fn get(&self, module: &str, name: &str) -> Option<&Import> {
        self.map.get(module).and_then(|m| m.get(name))
    }
}

impl ImportResolver for Imports {
    fn get(&self, module: &str, name: &str) -> Option<&Import> {
        self.get(module, name)
    }
}

pub trait ImportResolver {
    fn get(&self, module: &str, name: &str) -> Option<&Import>;
}
