mod recovery;
mod sighandler;

use crate::call::recovery::call_protected;
use wasmer_runtime::{
    backend::{Token, ProtectedCaller},
    types::{FuncIndex, Value, Type, FuncSig, LocalOrImport},
    module::ModuleInner,
    error::{RuntimeResult},
    export::Context,
    vm::{self, ImportBacking},
};
use libffi::high::{arg as libffi_arg, call as libffi_call, CodePtr};
use hashbrown::HashSet;
use std::iter;

pub struct Caller {
    func_export_set: HashSet<FuncIndex>,
}

impl Caller {
    pub fn new(module: &ModuleInner) -> Self {
        
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

        call_protected(|| {
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
                },
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
        LocalOrImport::Local(local_func_index) => {
            (
                module
                    .func_resolver
                    .get(&module, local_func_index)
                    .expect("broken invariant, func resolver not synced with module.exports")
                    .cast()
                    .as_ptr() as *const _,
                Context::Internal,
            )
        }
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