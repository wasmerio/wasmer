mod recovery;
mod sighandler;

pub use self::recovery::HandlerData;

use crate::call::recovery::call_protected;
use hashbrown::HashSet;
use libffi::high::{arg as libffi_arg, call as libffi_call, CodePtr};
use std::iter;
use wasmer_runtime::{
    backend::{ProtectedCaller, Token},
    error::RuntimeResult,
    export::Context,
    module::{ExportIndex, ModuleInner},
    types::{FuncIndex, FuncSig, LocalOrImport, Type, Value},
    vm::{self, ImportBacking},
};

pub struct Caller {
    func_export_set: HashSet<FuncIndex>,
    handler_data: HandlerData,
}

impl Caller {
    pub fn new(module: &ModuleInner, handler_data: HandlerData) -> Self {
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
        }
    }
}

impl ProtectedCaller for Caller {
    fn call(
        &self,
        module: &ModuleInner,
        func_index: FuncIndex,
        params: &[Value],
        returns: &mut [Value],
        import_backing: &ImportBacking,
        vmctx: *mut vm::Ctx,
        _: Token,
    ) -> RuntimeResult<()> {
        let (func_ptr, ctx, signature) = get_func_from_index(&module, import_backing, func_index);

        let vmctx_ptr = match ctx {
            Context::External(external_vmctx) => external_vmctx,
            Context::Internal => vmctx,
        };

        assert!(self.func_export_set.contains(&func_index));

        assert!(
            returns.len() == signature.returns.len() && signature.returns.len() <= 1,
            "multi-value returns not yet supported"
        );

        assert!(signature.check_sig(params), "incorrect signature");

        let libffi_args: Vec<_> = params
            .iter()
            .map(|val| match val {
                Value::I32(ref x) => libffi_arg(x),
                Value::I64(ref x) => libffi_arg(x),
                Value::F32(ref x) => libffi_arg(x),
                Value::F64(ref x) => libffi_arg(x),
            })
            .chain(iter::once(libffi_arg(&vmctx_ptr)))
            .collect();

        let code_ptr = CodePtr::from_ptr(func_ptr as _);

        call_protected(&self.handler_data, || {
            // Only supports zero or one return values for now.
            // To support multiple returns, we will have to
            // generate trampolines instead of using libffi.
            match signature.returns.first() {
                Some(ty) => {
                    let val = match ty {
                        Type::I32 => Value::I32(unsafe { libffi_call(code_ptr, &libffi_args) }),
                        Type::I64 => Value::I64(unsafe { libffi_call(code_ptr, &libffi_args) }),
                        Type::F32 => Value::F32(unsafe { libffi_call(code_ptr, &libffi_args) }),
                        Type::F64 => Value::F64(unsafe { libffi_call(code_ptr, &libffi_args) }),
                    };
                    returns[0] = val;
                }
                // call with no returns
                None => unsafe {
                    libffi_call::<()>(code_ptr, &libffi_args);
                },
            }
        })
    }
}

fn get_func_from_index<'a>(
    module: &'a ModuleInner,
    import_backing: &ImportBacking,
    func_index: FuncIndex,
) -> (*const vm::Func, Context, &'a FuncSig) {
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
            let imported_func = import_backing.imported_func(imported_func_index);
            (
                imported_func.func as *const _,
                Context::External(imported_func.vmctx),
            )
        }
    };

    let signature = module.sig_registry.lookup_func_sig(sig_index);

    (func_ptr, ctx, signature)
}
