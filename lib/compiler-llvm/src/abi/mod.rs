// LLVM implements part of the ABI lowering internally, but also requires that
// the user pack and unpack values themselves sometimes. This can help the LLVM
// optimizer by exposing operations to the optimizer, but it requires that the
// frontend know exactly what IR to produce in order to get the right ABI.

#![deny(dead_code, missing_docs)]

use crate::translator::intrinsics::Intrinsics;
use inkwell::{
    attributes::{Attribute, AttributeLoc},
    builder::Builder,
    context::Context,
    targets::TargetMachine,
    types::FunctionType,
    values::{BasicValueEnum, CallSiteValue, FunctionValue, IntValue, PointerValue},
};
use wasmer_types::CompileError;
use wasmer_types::FunctionType as FuncSig;
use wasmer_vm::VMOffsets;

mod aarch64_systemv;
mod x86_64_systemv;

use aarch64_systemv::Aarch64SystemV;
use x86_64_systemv::X86_64SystemV;

pub fn get_abi(target_machine: &TargetMachine) -> Box<dyn Abi> {
    if target_machine
        .get_triple()
        .as_str()
        .to_string_lossy()
        .starts_with("aarch64")
    {
        Box::new(Aarch64SystemV {})
    } else {
        Box::new(X86_64SystemV {})
    }
}

#[derive(Debug)]
pub(crate) enum G0M0FunctionKind {
    Local,
    Imported,
}

impl G0M0FunctionKind {
    /// Returns `true` if the function kind is [`Local`].
    ///
    /// [`Local`]: FunctionKind::Local
    #[must_use]
    pub(crate) fn is_local(&self) -> bool {
        matches!(self, Self::Local)
    }
}

/// The two additional parameters needed for g0m0 optimization.
pub(crate) type LocalFunctionG0M0params<'ctx> = Option<(IntValue<'ctx>, PointerValue<'ctx>)>;

/// We need to produce different LLVM IR for different platforms. (Contrary to
/// popular knowledge LLVM IR is not intended to be portable in that way.) This
/// trait deals with differences between function signatures on different
/// targets.
pub trait Abi {
    /// Given a function definition, retrieve the parameter that is the vmctx pointer.
    fn get_vmctx_ptr_param<'ctx>(&self, func_value: &FunctionValue<'ctx>) -> PointerValue<'ctx>;

    /// Given a function definition, retrieve the parameter that is the pointer to the first --
    /// number 0 -- local global.
    #[allow(unused)]
    fn get_g0_ptr_param<'ctx>(&self, func_value: &FunctionValue<'ctx>) -> IntValue<'ctx>;

    /// Given a function definition, retrieve the parameter that is the pointer to the first --
    /// number 0 -- local memory.
    ///
    /// # Notes
    /// This function assumes that g0m0 is enabled.
    fn get_m0_ptr_param<'ctx>(&self, func_value: &FunctionValue<'ctx>) -> PointerValue<'ctx>;

    /// Given a wasm function type, produce an llvm function declaration.
    ///
    /// # Notes
    /// This function assumes that g0m0 is enabled.
    fn func_type_to_llvm<'ctx>(
        &self,
        context: &'ctx Context,
        intrinsics: &Intrinsics<'ctx>,
        offsets: Option<&VMOffsets>,
        sig: &FuncSig,
        function_kind: Option<G0M0FunctionKind>,
    ) -> Result<(FunctionType<'ctx>, Vec<(Attribute, AttributeLoc)>), CompileError>;

    /// Marshall wasm stack values into function parameters.
    #[allow(clippy::too_many_arguments)]
    fn args_to_call<'ctx>(
        &self,
        alloca_builder: &Builder<'ctx>,
        func_sig: &FuncSig,
        llvm_fn_ty: &FunctionType<'ctx>,
        ctx_ptr: PointerValue<'ctx>,
        values: &[BasicValueEnum<'ctx>],
        intrinsics: &Intrinsics<'ctx>,
        g0m0: LocalFunctionG0M0params<'ctx>,
    ) -> Result<Vec<BasicValueEnum<'ctx>>, CompileError>;

    /// Given a CallSite, extract the returned values and return them in a Vec.
    fn rets_from_call<'ctx>(
        &self,
        builder: &Builder<'ctx>,
        intrinsics: &Intrinsics<'ctx>,
        call_site: CallSiteValue<'ctx>,
        func_sig: &FuncSig,
    ) -> Result<Vec<BasicValueEnum<'ctx>>, CompileError>;

    /// Whether the llvm equivalent of this wasm function has an `sret` attribute.
    fn is_sret(&self, func_sig: &FuncSig) -> Result<bool, CompileError>;

    /// Pack LLVM IR values representing individual wasm values into the return type for the function.
    fn pack_values_for_register_return<'ctx>(
        &self,
        intrinsics: &Intrinsics<'ctx>,
        builder: &Builder<'ctx>,
        values: &[BasicValueEnum<'ctx>],
        func_type: &FunctionType<'ctx>,
    ) -> Result<BasicValueEnum<'ctx>, CompileError>;
}
