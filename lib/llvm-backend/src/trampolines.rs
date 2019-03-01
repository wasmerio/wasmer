use inkwell::{
    builder::Builder,
    context::Context,
    module::{Linkage, Module},
    passes::PassManager,
    types::{BasicType, BasicTypeEnum, FunctionType, PointerType},
    values::{BasicValue, FunctionValue, PhiValue, PointerValue},
    AddressSpace, FloatPredicate, IntPredicate,
};
use wasmer_runtime_core::{
    module::ModuleInfo,
    types::{SigIndex, FuncSig},
    structures::{TypedIndex, SliceMap},
};
use crate::intrinsics::Intrinsics;

pub fn generate_trampolines(info: &ModuleInfo, signatures: &SliceMap<SigIndex, FunctionType>, module: &Module, builder: &Builder, intrinsics: &Intrinsics) -> Result<(), String> {
    let trampoline_sig = intrinsics.void_ty.fn_type(&[
        intrinsics.ctx_ptr_ty, // vmctx ptr
        intrinsics.i64_ptr_ty, // func ptr
        intrinsics.i64_ptr_ty,
        intrinsics.i64_ptr_ty,
    ], false);

    for (sig_index, sig) in info.signatures.iter() {

    }
}

pub fn generate_trampoline(sig_index: usize, trampoline_sig: FunctionType, sig: &FuncSig, builder: &Builder, intrinsics: &Intrinsics) {
    let function = module.add_function(
        &format!("tramp{}", sig_index.index()),
        signatures[sig_index],
        Some(Linkage::External),
    );


}