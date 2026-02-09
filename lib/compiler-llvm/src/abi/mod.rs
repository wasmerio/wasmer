// LLVM implements part of the ABI lowering internally, but also requires that
// the user pack and unpack values themselves sometimes. This can help the LLVM
// optimizer by exposing operations to the optimizer, but it requires that the
// frontend know exactly what IR to produce in order to get the right ABI.

#![deny(dead_code, missing_docs)]

use crate::error::err;
use crate::translator::intrinsics::{Intrinsics, type_to_llvm};
use inkwell::values::BasicValue;
use inkwell::{
    attributes::{Attribute, AttributeLoc},
    builder::Builder,
    context::Context,
    targets::TargetMachine,
    types::FunctionType,
    values::{BasicValueEnum, CallSiteValue, FunctionValue, IntValue, PointerValue},
};
use wasmer_types::{CompileError, FunctionType as FuncSig, Type};
use wasmer_vm::VMOffsets;

mod aarch64_systemv;
mod riscv_systemv;
mod x86_64_systemv;

use aarch64_systemv::Aarch64SystemV;
use riscv_systemv::RiscvSystemV;
use x86_64_systemv::X86_64SystemV;

pub fn get_abi(target_machine: &TargetMachine) -> Box<dyn Abi> {
    let target_name = target_machine.get_triple();
    let target_name = target_name.as_str().to_string_lossy();

    if target_name.starts_with("aarch64") {
        Box::new(Aarch64SystemV {})
    } else if target_name.starts_with("riscv") {
        Box::new(RiscvSystemV {
            is_riscv64: target_name.starts_with("riscv64"),
        })
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
    fn get_vmctx_ptr_param<'ctx>(&self, func_value: &FunctionValue<'ctx>) -> PointerValue<'ctx> {
        let param = func_value
            .get_nth_param(u32::from(
                func_value
                    .get_enum_attribute(
                        AttributeLoc::Param(0),
                        Attribute::get_named_enum_kind_id("sret"),
                    )
                    .is_some(),
            ))
            .unwrap();
        //param.set_name("vmctx");

        param.into_pointer_value()
    }

    /// Given a function definition, retrieve the parameter that is the pointer to the first --
    /// number 0 -- local global.
    #[allow(unused)]
    fn get_g0_ptr_param<'ctx>(&self, func_value: &FunctionValue<'ctx>) -> IntValue<'ctx> {
        // g0 is always after the vmctx.
        let vmctx_idx = u32::from(
            func_value
                .get_enum_attribute(
                    AttributeLoc::Param(0),
                    Attribute::get_named_enum_kind_id("sret"),
                )
                .is_some(),
        );

        let param = func_value.get_nth_param(vmctx_idx + 1).unwrap();
        param.set_name("g0");

        param.into_int_value()
    }

    /// Given a function definition, retrieve the parameter that is the pointer to the first --
    /// number 0 -- local memory.
    ///
    /// # Notes
    /// This function assumes that g0m0 is enabled.
    fn get_m0_ptr_param<'ctx>(&self, func_value: &FunctionValue<'ctx>) -> PointerValue<'ctx> {
        // m0 is always after g0.
        let vmctx_idx = u32::from(
            func_value
                .get_enum_attribute(
                    AttributeLoc::Param(0),
                    Attribute::get_named_enum_kind_id("sret"),
                )
                .is_some(),
        );

        let param = func_value.get_nth_param(vmctx_idx + 2).unwrap();
        param.set_name("m0_base_ptr");

        param.into_pointer_value()
    }

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
    ) -> Result<Vec<BasicValueEnum<'ctx>>, CompileError> {
        // If it's an sret, allocate the return space.
        let sret = if llvm_fn_ty.get_return_type().is_none() && func_sig.results().len() > 1 {
            let llvm_params: Vec<_> = func_sig
                .results()
                .iter()
                .map(|x| type_to_llvm(intrinsics, *x).unwrap())
                .collect();
            let llvm_params = llvm_fn_ty
                .get_context()
                .struct_type(llvm_params.as_slice(), false);
            Some(err!(alloca_builder.build_alloca(llvm_params, "sret")))
        } else {
            None
        };

        let mut args = vec![ctx_ptr.as_basic_value_enum()];

        if let Some((g0, m0)) = g0m0 {
            args.push(g0.into());
            args.push(m0.into());
        }

        let args = args.into_iter().chain(values.iter().copied());

        let ret = if let Some(sret) = sret {
            std::iter::once(sret.as_basic_value_enum())
                .chain(args)
                .collect()
        } else {
            args.collect()
        };

        Ok(ret)
    }

    /// Given a CallSite, extract the returned values and return them in a Vec.
    fn rets_from_call<'ctx>(
        &self,
        builder: &Builder<'ctx>,
        intrinsics: &Intrinsics<'ctx>,
        call_site: CallSiteValue<'ctx>,
        func_sig: &FuncSig,
    ) -> Result<Vec<BasicValueEnum<'ctx>>, CompileError>;

    /// Whether the llvm equivalent of this wasm function has an `sret` attribute.
    fn is_sret(&self, func_sig: &FuncSig) -> Result<bool, CompileError> {
        let func_sig_returns_bitwidths = func_sig
            .results()
            .iter()
            .map(|ty| match ty {
                Type::I32 | Type::F32 | Type::ExceptionRef => 32,
                Type::I64 | Type::F64 => 64,
                Type::V128 => 128,
                Type::ExternRef | Type::FuncRef => 64, /* pointer */
            })
            .collect::<Vec<i32>>();

        Ok(!matches!(
            func_sig_returns_bitwidths.as_slice(),
            [] | [_]
                | [32, 32]
                | [32, 64]
                | [64, 32]
                | [64, 64]
                | [32, 32, 32]
                | [32, 32, 64]
                | [64, 32, 32]
                | [32, 32, 32, 32]
        ))
    }

    /// Pack LLVM IR values representing individual wasm values into the return type for the function.
    fn pack_values_for_register_return<'ctx>(
        &self,
        intrinsics: &Intrinsics<'ctx>,
        builder: &Builder<'ctx>,
        values: &[BasicValueEnum<'ctx>],
        func_type: &FunctionType<'ctx>,
    ) -> Result<BasicValueEnum<'ctx>, CompileError>;
}
