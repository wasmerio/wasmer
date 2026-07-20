// LLVM implements part of the ABI lowering internally, but also requires that
// the user pack and unpack values themselves sometimes.

#![deny(missing_docs)]

use crate::error::{err, err_nt};
use crate::translator::intrinsics::{Intrinsics, type_to_llvm};
use inkwell::{
    AddressSpace,
    attributes::{Attribute, AttributeLoc},
    builder::Builder,
    context::Context,
    targets::TargetMachine,
    types::{
        AnyType, BasicMetadataTypeEnum, BasicType, BasicTypeEnum, FunctionType, StructType,
    },
    values::{
        BasicValue, BasicValueEnum, CallSiteValue, FloatValue, FunctionValue, IntValue,
        PointerValue, VectorValue,
    },
};
use itertools::Itertools;
use wasmer_compiler::abi::ReturnAbi;
use wasmer_types::{CompileError, FunctionType as FuncSig, Type};
use wasmer_vm::VMOffsets;

mod aarch64_systemv;
mod riscv_systemv;
mod x86_64_systemv;

use aarch64_systemv::Aarch64SystemV;
use riscv_systemv::RiscvSystemV;
use x86_64_systemv::X86_64SystemV;

/// Target-specific return-value classification.
pub(crate) trait Architecture {
    /// Classifies a WebAssembly function's return values.
    fn classify_return_type(&self, types: &[Type]) -> ReturnAbi;

    /// Whether i32 parameters require RISC-V's sign-extension attributes.
    fn sign_extend_i32_params(&self) -> bool {
        false
    }
}

/// Architectures supported by the LLVM backend.
pub(crate) enum TargetArchitecture {
    X86_64(X86_64SystemV),
    Aarch64(Aarch64SystemV),
    Riscv(RiscvSystemV),
}

impl Architecture for TargetArchitecture {
    fn classify_return_type(&self, types: &[Type]) -> ReturnAbi {
        match self {
            Self::X86_64(arch) => arch.classify_return_type(types),
            Self::Aarch64(arch) => arch.classify_return_type(types),
            Self::Riscv(arch) => arch.classify_return_type(types),
        }
    }

    fn sign_extend_i32_params(&self) -> bool {
        matches!(self, Self::Riscv(arch) if arch.is_riscv64)
    }
}

/// LLVM ABI lowering shared by all supported architectures.
pub(crate) struct LLVMAbi<A: Architecture = TargetArchitecture> {
    pub(crate) architecture: A,
}

/// Selects ABI lowering for an LLVM target machine.
pub(crate) fn get_abi(target_machine: &TargetMachine) -> LLVMAbi<TargetArchitecture> {
    let target_name = target_machine.get_triple();
    let target_name = target_name.as_str().to_string_lossy();
    let architecture = if target_name.starts_with("aarch64") {
        TargetArchitecture::Aarch64(Aarch64SystemV)
    } else if target_name.starts_with("riscv") {
        TargetArchitecture::Riscv(RiscvSystemV {
            is_riscv64: target_name.starts_with("riscv64"),
        })
    } else {
        TargetArchitecture::X86_64(X86_64SystemV)
    };
    LLVMAbi { architecture }
}

/// We need to produce different LLVM IR for different platforms. (Contrary to
/// popular knowledge LLVM IR is not intended to be portable in that way.) This
/// trait deals with differences between function signatures on different
/// targets.
impl<A: Architecture> LLVMAbi<A> {
    /// Given a function definition, retrieve the parameter that is the vmctx pointer.
    pub(crate) fn get_vmctx_ptr_param<'ctx>(
        &self,
        func_value: &FunctionValue<'ctx>,
    ) -> PointerValue<'ctx> {
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
        param.set_name("vmctx");

        param.into_pointer_value()
    }

    /// Given a function definition, retrieve the parameter that is the pointer to the first --
    /// number 0 -- local memory.
    pub(crate) fn get_m0_ptr_param<'ctx>(
        &self,
        func_value: &FunctionValue<'ctx>,
    ) -> PointerValue<'ctx> {
        let vmctx_idx = u32::from(
            func_value
                .get_enum_attribute(
                    AttributeLoc::Param(0),
                    Attribute::get_named_enum_kind_id("sret"),
                )
                .is_some(),
        );

        let param = func_value.get_nth_param(vmctx_idx + 1).unwrap();
        param.set_name("m0_base_ptr");

        param.into_pointer_value()
    }

    /// Marshall wasm stack values into function parameters.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn args_to_call<'ctx>(
        &self,
        alloca_builder: &Builder<'ctx>,
        func_sig: &FuncSig,
        llvm_fn_ty: &FunctionType<'ctx>,
        ctx_ptr: PointerValue<'ctx>,
        values: &[BasicValueEnum<'ctx>],
        intrinsics: &Intrinsics<'ctx>,
        m0: Option<PointerValue<'ctx>>,
        sret_ptr: Option<PointerValue<'ctx>>,
    ) -> Result<Vec<BasicValueEnum<'ctx>>, CompileError> {
        // If it's an sret, allocate the return space.
        let sret = if self.llvm_fn_uses_sret(llvm_fn_ty, func_sig) {
            let llvm_params: Vec<_> = func_sig
                .results()
                .iter()
                .map(|x| type_to_llvm(intrinsics, *x).unwrap())
                .collect();
            let llvm_params = llvm_fn_ty
                .get_context()
                .struct_type(llvm_params.as_slice(), false);
            // If return_call is used, we pass existing sret pointer instead a newly created one.
            Some(match sret_ptr {
                Some(sret_ptr) => sret_ptr,
                None => err!(alloca_builder.build_alloca(llvm_params, "sret")),
            })
        } else {
            None
        };

        let mut args = vec![ctx_ptr.as_basic_value_enum()];

        if let Some(m0) = m0 {
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

    /// Whether a concrete LLVM function type uses an `sret` parameter for the given wasm signature.
    pub(crate) fn llvm_fn_uses_sret<'ctx>(
        &self,
        llvm_fn_ty: &FunctionType<'ctx>,
        func_sig: &FuncSig,
    ) -> bool {
        llvm_fn_ty.get_return_type().is_none() && func_sig.results().len() > 1
    }

    /// Whether the native function uses an `sret` parameter.
    pub(crate) fn is_sret(&self, func_sig: &FuncSig) -> Result<bool, CompileError> {
        Ok(matches!(
            self.architecture.classify_return_type(func_sig.results()),
            ReturnAbi::Sret(_)
        ))
    }
}

impl<A: Architecture> LLVMAbi<A> {
    // Given a wasm function type, produce an llvm function declaration.
    pub(crate) fn func_type_to_llvm<'ctx>(
        &self,
        context: &'ctx Context,
        intrinsics: &Intrinsics<'ctx>,
        offsets: Option<&VMOffsets>,
        sig: &FuncSig,
        include_m0_param: bool,
    ) -> Result<(FunctionType<'ctx>, Vec<(Attribute, AttributeLoc)>), CompileError> {
        let type_for_pair = |t0: Type, t1: Type| {
            if t0 == Type::F32 && t1 == Type::F32 {
                intrinsics.f32_ty.vec_type(2).as_basic_type_enum()
            } else {
                intrinsics.i64_ty.as_basic_type_enum()
            }
        };

        let return_abi = self.architecture.classify_return_type(sig.results());
        let return_llvm_type: Option<BasicTypeEnum<'ctx>> = match &return_abi {
            ReturnAbi::Void | ReturnAbi::Sret(_) => None,
            ReturnAbi::Single(single_value) => Some(type_to_llvm(intrinsics, *single_value)?),
            ReturnAbi::Pair(t0, t1) => Some(
                context
                    .struct_type(
                        &[
                            type_to_llvm(intrinsics, *t0)?,
                            type_to_llvm(intrinsics, *t1)?,
                        ],
                        false,
                    )
                    .as_basic_type_enum(),
            ),
            ReturnAbi::Unpacked(types) => Some(
                context
                    .struct_type(
                        &types
                            .iter()
                            .map(|ty| type_to_llvm(intrinsics, *ty))
                            .collect::<Result<Vec<_>, _>>()?,
                        false,
                    )
                    .as_basic_type_enum(),
            ),
            ReturnAbi::PackedPair(t0, t1) => Some(type_for_pair(*t0, *t1)),
            ReturnAbi::PackedFirst(t0, t1, t2) => Some(
                context
                    .struct_type(
                        &[type_for_pair(*t0, *t1), type_to_llvm(intrinsics, *t2)?],
                        false,
                    )
                    .as_basic_type_enum(),
            ),
            ReturnAbi::PackedLast(t0, t1, t2) => Some(
                context
                    .struct_type(
                        &[type_to_llvm(intrinsics, *t0)?, type_for_pair(*t1, *t2)],
                        false,
                    )
                    .as_basic_type_enum(),
            ),
            ReturnAbi::PackedQuads(t0, t1, t2, t3) => Some(
                context
                    .struct_type(&[type_for_pair(*t0, *t1), type_for_pair(*t2, *t3)], false)
                    .as_basic_type_enum(),
            ),
        };

        let user_param_types = sig.params().iter().map(|&ty| type_to_llvm(intrinsics, ty));
        let mut param_types = vec![Ok(intrinsics.ptr_ty.as_basic_type_enum())];
        if include_m0_param {
            param_types.push(Ok(intrinsics.ptr_ty.as_basic_type_enum()));
        }
        let param_llvm_types = param_types
            .into_iter()
            .chain(user_param_types)
            .map(|v| v.map(Into::into))
            .collect::<Result<Vec<BasicMetadataTypeEnum>, _>>()?;

        // TODO: figure out how many bytes long vmctx is, and mark it dereferenceable. (no need to mark it nonnull once we do this.)
        let vmctx_attributes = |i: u32| {
            vec![
                (
                    context.create_enum_attribute(Attribute::get_named_enum_kind_id("nofree"), 0),
                    AttributeLoc::Param(i),
                ),
                (
                    if let Some(offsets) = offsets {
                        context.create_enum_attribute(
                            Attribute::get_named_enum_kind_id("dereferenceable"),
                            offsets.size_of_vmctx().into(),
                        )
                    } else {
                        context
                            .create_enum_attribute(Attribute::get_named_enum_kind_id("nonnull"), 0)
                    },
                    AttributeLoc::Param(i),
                ),
                (
                    context.create_enum_attribute(
                        Attribute::get_named_enum_kind_id("align"),
                        std::mem::align_of::<wasmer_vm::VMContext>()
                            .try_into()
                            .unwrap(),
                    ),
                    AttributeLoc::Param(i),
                ),
            ]
        };

        let (function_type, mut attributes, sret_param) =
            if let ReturnAbi::Sret(types) = &return_abi {
                let basic_types = types
                    .iter()
                    .map(|&ty| type_to_llvm(intrinsics, ty))
                    .collect::<Result<Vec<_>, _>>()?;
                let sret = context.struct_type(&basic_types, false);
                let sret_ptr = context.ptr_type(AddressSpace::default());
                let sret_param_llvm_types =
                    std::iter::once(BasicMetadataTypeEnum::from(sret_ptr.as_basic_type_enum()))
                        .chain(param_llvm_types.iter().copied())
                        .collect_vec();

                let mut attributes = vec![(
                    context.create_type_attribute(
                        Attribute::get_named_enum_kind_id("sret"),
                        sret.as_any_type_enum(),
                    ),
                    AttributeLoc::Param(0),
                )];
                attributes.append(&mut vmctx_attributes(1));
                (
                    intrinsics
                        .void_ty
                        .fn_type(sret_param_llvm_types.as_slice(), false),
                    attributes,
                    1,
                )
            } else {
                let function_type = match return_llvm_type {
                    Some(return_type) => return_type.fn_type(param_llvm_types.as_slice(), false),
                    None => intrinsics
                        .void_ty
                        .fn_type(param_llvm_types.as_slice(), false),
                };
                (function_type, vmctx_attributes(0), 0)
            };

        if self.architecture.sign_extend_i32_params() {
            let extra_params = 1 + usize::from(include_m0_param) + sret_param;
            for (index, ty) in sig.params().iter().enumerate() {
                if *ty == Type::I32 {
                    for name in ["signext", "noundef"] {
                        attributes.push((
                            context
                                .create_enum_attribute(Attribute::get_named_enum_kind_id(name), 0),
                            AttributeLoc::Param((index + extra_params) as u32),
                        ));
                    }
                }
            }
        }

        Ok((function_type, attributes))
    }

    // Given a CallSite, extract the returned values and return them in a Vec.
    pub(crate) fn rets_from_call<'ctx>(
        &self,
        builder: &Builder<'ctx>,
        intrinsics: &Intrinsics<'ctx>,
        call_site: CallSiteValue<'ctx>,
        func_sig: &FuncSig,
    ) -> Result<Vec<BasicValueEnum<'ctx>>, CompileError> {
        let split_i64 =
            |value: IntValue<'ctx>| -> Result<(IntValue<'ctx>, IntValue<'ctx>), CompileError> {
                assert!(value.get_type() == intrinsics.i64_ty);
                let low = err!(builder.build_int_truncate(value, intrinsics.i32_ty, ""));
                let lshr = err!(builder.build_right_shift(
                    value,
                    intrinsics.i64_ty.const_int(32, false),
                    false,
                    ""
                ));
                let high = err!(builder.build_int_truncate(lshr, intrinsics.i32_ty, ""));
                Ok((low, high))
            };

        let f32x2_ty = intrinsics.f32_ty.vec_type(2).as_basic_type_enum();
        let extract_f32x2 = |value: VectorValue<'ctx>| -> Result<(FloatValue<'ctx>, FloatValue<'ctx>), CompileError> {
            assert!(value.get_type() == f32x2_ty.into_vector_type());
            let ret0 = err!(builder
                .build_extract_element(value, intrinsics.i32_ty.const_int(0, false), ""))
                .into_float_value();
            let ret1 = err!(builder
                .build_extract_element(value, intrinsics.i32_ty.const_int(1, false), ""))
                .into_float_value();
            Ok((ret0, ret1))
        };

        let casted =
            |value: BasicValueEnum<'ctx>, ty: Type| -> Result<BasicValueEnum<'ctx>, CompileError> {
                match ty {
                    Type::I32 | Type::ExceptionRef => {
                        assert!(
                            value.get_type() == intrinsics.i32_ty.as_basic_type_enum()
                                || value.get_type() == intrinsics.f32_ty.as_basic_type_enum()
                        );
                        err_nt!(builder.build_bit_cast(value, intrinsics.i32_ty, ""))
                    }
                    Type::F32 => {
                        assert!(
                            value.get_type() == intrinsics.i32_ty.as_basic_type_enum()
                                || value.get_type() == intrinsics.f32_ty.as_basic_type_enum()
                        );
                        err_nt!(builder.build_bit_cast(value, intrinsics.f32_ty, ""))
                    }
                    _ => panic!("should only be called to repack 32-bit values"),
                }
            };

        if let Some(basic_value) = call_site.try_as_basic_value().basic() {
            if func_sig.results().len() > 1 {
                if basic_value.get_type() == intrinsics.i64_ty.as_basic_type_enum() {
                    assert!(func_sig.results().len() == 2);
                    let value = basic_value.into_int_value();
                    let (low, high) = split_i64(value)?;
                    let low = casted(low.into(), func_sig.results()[0])?;
                    let high = casted(high.into(), func_sig.results()[1])?;
                    return Ok(vec![low, high]);
                }
                if basic_value.get_type() == f32x2_ty {
                    assert!(func_sig.results().len() == 2);
                    let (ret0, ret1) = extract_f32x2(basic_value.into_vector_value())?;
                    return Ok(vec![ret0.into(), ret1.into()]);
                }
                let struct_value = basic_value.into_struct_value();
                let rets = (0..struct_value.get_type().count_fields())
                    .map(|i| builder.build_extract_value(struct_value, i, "").unwrap())
                    .collect_vec();
                let ret = match self.architecture.classify_return_type(func_sig.results()) {
                    ReturnAbi::Pair(_, _) | ReturnAbi::Unpacked(_) => rets,
                    ReturnAbi::PackedFirst(Type::F32, Type::F32, _) => {
                        assert!(func_sig.results().len() == 3);
                        let (rets0, rets1) = extract_f32x2(rets[0].into_vector_value())?;
                        vec![rets0.into(), rets1.into(), rets[1]]
                    }
                    ReturnAbi::PackedFirst(t0, t1, _) => {
                        assert!(func_sig.results().len() == 3);
                        let (low, high) = split_i64(rets[0].into_int_value())?;
                        let low = casted(low.into(), t0)?;
                        let high = casted(high.into(), t1)?;
                        vec![low, high, rets[1]]
                    }
                    ReturnAbi::PackedLast(_, Type::F32, Type::F32) => {
                        assert!(func_sig.results().len() == 3);
                        let (rets1, rets2) = extract_f32x2(rets[1].into_vector_value())?;
                        vec![rets[0], rets1.into(), rets2.into()]
                    }
                    ReturnAbi::PackedLast(_, t1, t2) => {
                        assert!(func_sig.results().len() == 3);
                        let (rets1, rets2) = split_i64(rets[1].into_int_value())?;
                        let rets1 = casted(rets1.into(), t1)?;
                        let rets2 = casted(rets2.into(), t2)?;
                        vec![rets[0], rets1, rets2]
                    }
                    ReturnAbi::PackedQuads(t0, t1, t2, t3) => {
                        assert!(func_sig.results().len() == 4);
                        let (low0, high0) = if rets[0].get_type()
                            == intrinsics.f32_ty.vec_type(2).as_basic_type_enum()
                        {
                            let (x, y) = extract_f32x2(rets[0].into_vector_value())?;
                            (x.into(), y.into())
                        } else {
                            let (x, y) = split_i64(rets[0].into_int_value())?;
                            (x.into(), y.into())
                        };
                        let (low1, high1) = if rets[1].get_type()
                            == intrinsics.f32_ty.vec_type(2).as_basic_type_enum()
                        {
                            let (x, y) = extract_f32x2(rets[1].into_vector_value())?;
                            (x.into(), y.into())
                        } else {
                            let (x, y) = split_i64(rets[1].into_int_value())?;
                            (x.into(), y.into())
                        };
                        let low0 = casted(low0, t0)?;
                        let high0 = casted(high0, t1)?;
                        let low1 = casted(low1, t2)?;
                        let high1 = casted(high1, t3)?;
                        vec![low0, high0, low1, high1]
                    }
                    ReturnAbi::Void
                    | ReturnAbi::Single(_)
                    | ReturnAbi::PackedPair(_, _)
                    | ReturnAbi::Sret(_) => {
                        unreachable!("expected an sret for this type")
                    }
                };

                Ok(ret)
            } else {
                assert!(func_sig.results().len() == 1);
                Ok(vec![basic_value])
            }
        } else {
            assert!(call_site.count_arguments() > 0); // Either sret or vmctx.
            if call_site
                .get_enum_attribute(
                    AttributeLoc::Param(0),
                    Attribute::get_named_enum_kind_id("sret"),
                )
                .is_some()
            {
                let sret_ty = call_site
                    .try_as_basic_value()
                    .unwrap_instruction()
                    .get_operand(0)
                    .unwrap()
                    .unwrap_value();
                let sret = sret_ty.into_pointer_value();
                // re-build the llvm-type struct holding the return values
                let llvm_results: Vec<_> = func_sig
                    .results()
                    .iter()
                    .map(|x| type_to_llvm(intrinsics, *x).unwrap())
                    .collect();
                let struct_type = intrinsics
                    .i32_ty
                    .get_context()
                    .struct_type(llvm_results.as_slice(), false);

                let struct_value =
                    err!(builder.build_load(struct_type, sret, "")).into_struct_value();
                let mut rets: Vec<_> = Vec::new();
                for i in 0..struct_value.get_type().count_fields() {
                    let value = builder.build_extract_value(struct_value, i, "").unwrap();
                    rets.push(value);
                }
                assert!(func_sig.results().len() == rets.len());
                Ok(rets)
            } else {
                assert!(func_sig.results().is_empty());
                Ok(vec![])
            }
        }
    }

    pub(crate) fn pack_values_for_register_return<'ctx>(
        &self,
        intrinsics: &Intrinsics<'ctx>,
        builder: &Builder<'ctx>,
        values: &[BasicValueEnum<'ctx>],
        func_sig: &FuncSig,
        func_type: &FunctionType<'ctx>,
    ) -> Result<BasicValueEnum<'ctx>, CompileError> {
        let pack_i32s = |low: BasicValueEnum<'ctx>, high: BasicValueEnum<'ctx>| {
            assert!(low.get_type() == intrinsics.i32_ty.as_basic_type_enum());
            assert!(high.get_type() == intrinsics.i32_ty.as_basic_type_enum());
            let (low, high) = (low.into_int_value(), high.into_int_value());
            let low = err!(builder.build_int_z_extend(low, intrinsics.i64_ty, ""));
            let high = err!(builder.build_int_z_extend(high, intrinsics.i64_ty, ""));
            let high =
                err!(builder.build_left_shift(high, intrinsics.i64_ty.const_int(32, false), ""));
            err_nt!(
                builder
                    .build_or(low, high, "")
                    .map(|v| v.as_basic_value_enum())
            )
        };

        let pack_f32s = |first: BasicValueEnum<'ctx>,
                         second: BasicValueEnum<'ctx>|
         -> Result<BasicValueEnum<'ctx>, CompileError> {
            assert!(first.get_type() == intrinsics.f32_ty.as_basic_type_enum());
            assert!(second.get_type() == intrinsics.f32_ty.as_basic_type_enum());
            let (first, second) = (first.into_float_value(), second.into_float_value());
            let vec_ty = intrinsics.f32_ty.vec_type(2);
            let vec = err!(builder.build_insert_element(
                vec_ty.get_undef(),
                first,
                intrinsics.i32_zero,
                ""
            ));
            err_nt!(
                builder
                    .build_insert_element(vec, second, intrinsics.i32_ty.const_int(1, false), "")
                    .map(|v| v.as_basic_value_enum())
            )
        };

        let build_struct = |ty: StructType<'ctx>, values: &[BasicValueEnum<'ctx>]| {
            let mut struct_value = ty.get_undef();
            for (i, v) in values.iter().enumerate() {
                struct_value = builder
                    .build_insert_value(struct_value, *v, i as u32, "")
                    .unwrap()
                    .into_struct_value();
            }
            struct_value.as_basic_value_enum()
        };

        let return_abi = self.architecture.classify_return_type(func_sig.results());

        Ok(match return_abi {
            ReturnAbi::Single(_) => values[0],
            ReturnAbi::PackedPair(Type::F32, Type::F32) => pack_f32s(values[0], values[1])?,
            ReturnAbi::PackedPair(_, _) => {
                let v1 = err!(builder.build_bit_cast(values[0], intrinsics.i32_ty, ""));
                let v2 = err!(builder.build_bit_cast(values[1], intrinsics.i32_ty, ""));
                pack_i32s(v1, v2)?
            }
            ReturnAbi::Pair(_, _) | ReturnAbi::Unpacked(_) => build_struct(
                func_type.get_return_type().unwrap().into_struct_type(),
                values,
            ),
            ReturnAbi::PackedFirst(Type::F32, Type::F32, _) => build_struct(
                func_type.get_return_type().unwrap().into_struct_type(),
                &[pack_f32s(values[0], values[1])?, values[2]],
            ),
            ReturnAbi::PackedFirst(_, _, _) => {
                let v1 = err!(builder.build_bit_cast(values[0], intrinsics.i32_ty, ""));
                let v2 = err!(builder.build_bit_cast(values[1], intrinsics.i32_ty, ""));
                build_struct(
                    func_type.get_return_type().unwrap().into_struct_type(),
                    &[pack_i32s(v1, v2)?, values[2]],
                )
            }
            ReturnAbi::PackedLast(_, Type::F32, Type::F32) => build_struct(
                func_type.get_return_type().unwrap().into_struct_type(),
                &[values[0], pack_f32s(values[1], values[2])?],
            ),
            ReturnAbi::PackedLast(_, _, _) => {
                let v2 = err!(builder.build_bit_cast(values[1], intrinsics.i32_ty, ""));
                let v3 = err!(builder.build_bit_cast(values[2], intrinsics.i32_ty, ""));
                build_struct(
                    func_type.get_return_type().unwrap().into_struct_type(),
                    &[values[0], pack_i32s(v2, v3)?],
                )
            }
            ReturnAbi::PackedQuads(t0, t1, t2, t3) => {
                let v1v2_pack = if t0 == Type::F32 && t1 == Type::F32 {
                    pack_f32s(values[0], values[1])?
                } else {
                    let v1 = err!(builder.build_bit_cast(values[0], intrinsics.i32_ty, ""));
                    let v2 = err!(builder.build_bit_cast(values[1], intrinsics.i32_ty, ""));
                    pack_i32s(v1, v2)?
                };
                let v3v4_pack = if t2 == Type::F32 && t3 == Type::F32 {
                    pack_f32s(values[2], values[3])?
                } else {
                    let v3 = err!(builder.build_bit_cast(values[2], intrinsics.i32_ty, ""));
                    let v4 = err!(builder.build_bit_cast(values[3], intrinsics.i32_ty, ""));
                    pack_i32s(v3, v4)?
                };
                build_struct(
                    func_type.get_return_type().unwrap().into_struct_type(),
                    &[v1v2_pack, v3v4_pack],
                )
            }
            ReturnAbi::Void | ReturnAbi::Sret(_) => {
                unreachable!("called to perform register return on struct return or void function")
            }
        })
    }
}
