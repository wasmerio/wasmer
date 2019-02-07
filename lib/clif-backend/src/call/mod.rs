mod recovery;
mod sighandler;

pub use self::recovery::{call_protected, HandlerData};

use crate::trampoline::Trampolines;

use hashbrown::HashSet;
use std::sync::Arc;
use wasmer_runtime_core::{
    backend::{ProtectedCaller, Token},
    error::RuntimeResult,
    export::Context,
    module::{ExportIndex, ModuleInfo, ModuleInner},
    types::{FuncIndex, FuncSig, LocalOrImport, SigIndex, Type, Value},
    vm::{self, ImportBacking},
};

pub struct Caller {
    func_export_set: HashSet<FuncIndex>,
    handler_data: HandlerData,
    trampolines: Trampolines,
}

impl Caller {
    pub fn new(module: &ModuleInfo, handler_data: HandlerData, trampolines: Trampolines) -> Self {
        let mut func_export_set = HashSet::new();
        for export_index in module.exports.values() {
            if let ExportIndex::Func(func_index) = export_index {
                func_export_set.insert(*func_index);
            }
        }
        if let Some(start_func_index) = module.start_func {
            func_export_set.insert(start_func_index);
        }

        Self {
            func_export_set,
            handler_data,
            trampolines,
        }
    }
}

impl ProtectedCaller for Caller {
    fn call(
        &self,
        module: &ModuleInner,
        func_index: FuncIndex,
        params: &[Value],
        import_backing: &ImportBacking,
        vmctx: *mut vm::Ctx,
        _: Token,
    ) -> RuntimeResult<Vec<Value>> {
        let (func_ptr, ctx, signature, sig_index) =
            get_func_from_index(&module, import_backing, func_index);

        let vmctx_ptr = match ctx {
            Context::External(external_vmctx) => external_vmctx,
            Context::Internal => vmctx,
        };

        assert!(self.func_export_set.contains(&func_index));

        assert!(
            signature.returns().len() <= 1,
            "multi-value returns not yet supported"
        );

        assert!(
            signature.check_param_value_types(params),
            "incorrect signature"
        );

        let param_vec: Vec<u64> = params
            .iter()
            .map(|val| match val {
                Value::I32(x) => *x as u64,
                Value::I64(x) => *x as u64,
                Value::F32(x) => x.to_bits() as u64,
                Value::F64(x) => x.to_bits(),
            })
            .collect();

        let mut return_vec = vec![0; signature.returns().len()];

        let trampoline = self
            .trampolines
            .lookup(sig_index)
            .expect("that trampoline doesn't exist");

        call_protected(&self.handler_data, || unsafe {
            // Leap of faith.
            trampoline(
                vmctx_ptr,
                func_ptr,
                param_vec.as_ptr(),
                return_vec.as_mut_ptr(),
            );
        })?;

        Ok(return_vec
            .iter()
            .zip(signature.returns().iter())
            .map(|(&x, ty)| match ty {
                Type::I32 => Value::I32(x as i32),
                Type::I64 => Value::I64(x as i64),
                Type::F32 => Value::F32(f32::from_bits(x as u32)),
                Type::F64 => Value::F64(f64::from_bits(x as u64)),
            })
            .collect())
    }
}

fn get_func_from_index(
    module: &ModuleInner,
    import_backing: &ImportBacking,
    func_index: FuncIndex,
) -> (*const vm::Func, Context, Arc<FuncSig>, SigIndex) {
    let sig_index = *module
        .info
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
            let imported_func = import_backing.imported_func(imported_func_index);
            (
                imported_func.func as *const _,
                Context::External(imported_func.vmctx),
            )
        }
    };

    let signature = Arc::clone(&module.info.signatures[sig_index]);

    (func_ptr, ctx, signature, sig_index)
}
