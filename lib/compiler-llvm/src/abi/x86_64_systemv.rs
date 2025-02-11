use crate::abi::Abi;
use crate::error::{err, err_nt};
use crate::translator::intrinsics::{type_to_llvm, Intrinsics};
use inkwell::{
    attributes::{Attribute, AttributeLoc},
    builder::Builder,
    context::Context,
    types::{AnyType, BasicMetadataTypeEnum, BasicType, FunctionType, StructType},
    values::{
        BasicValue, BasicValueEnum, CallSiteValue, FloatValue, FunctionValue, IntValue,
        PointerValue, VectorValue,
    },
    AddressSpace,
};
use wasmer_types::{CompileError, FunctionType as FuncSig, Type};
use wasmer_vm::VMOffsets;

use std::convert::TryInto;

/// Implementation of the [`Abi`] trait for the AMD64 SystemV ABI.
pub struct X86_64SystemV {}

impl Abi for X86_64SystemV {
    // Given a function definition, retrieve the parameter that is the vmctx pointer.
    fn get_vmctx_ptr_param<'ctx>(&self, func_value: &FunctionValue<'ctx>) -> PointerValue<'ctx> {
        func_value
            .get_nth_param(u32::from(
                func_value
                    .get_enum_attribute(
                        AttributeLoc::Param(0),
                        Attribute::get_named_enum_kind_id("sret"),
                    )
                    .is_some(),
            ))
            .unwrap()
            .into_pointer_value()
    }

    // Given a wasm function type, produce an llvm function declaration.
    fn func_type_to_llvm<'ctx>(
        &self,
        context: &'ctx Context,
        intrinsics: &Intrinsics<'ctx>,
        offsets: Option<&VMOffsets>,
        sig: &FuncSig,
    ) -> Result<(FunctionType<'ctx>, Vec<(Attribute, AttributeLoc)>), CompileError> {
        let user_param_types = sig.params().iter().map(|&ty| type_to_llvm(intrinsics, ty));

        let param_types =
            std::iter::once(Ok(intrinsics.ptr_ty.as_basic_type_enum())).chain(user_param_types);

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

        let sig_returns_bitwidths = sig
            .results()
            .iter()
            .map(|ty| match ty {
                Type::I32 | Type::F32 => 32,
                Type::I64 | Type::F64 => 64,
                Type::V128 => 128,
                Type::ExternRef | Type::FuncRef | Type::ExceptionRef => 64, /* pointer */
            })
            .collect::<Vec<i32>>();

        Ok(match sig_returns_bitwidths.as_slice() {
            [] => (
                intrinsics.void_ty.fn_type(
                    param_types
                        .map(|v| v.map(Into::into))
                        .collect::<Result<Vec<BasicMetadataTypeEnum>, _>>()?
                        .as_slice(),
                    false,
                ),
                vmctx_attributes(0),
            ),
            [_] => {
                let single_value = sig.results()[0];
                (
                    type_to_llvm(intrinsics, single_value)?.fn_type(
                        param_types
                            .map(|v| v.map(Into::into))
                            .collect::<Result<Vec<BasicMetadataTypeEnum>, _>>()?
                            .as_slice(),
                        false,
                    ),
                    vmctx_attributes(0),
                )
            }
            [32, 64] | [64, 32] | [64, 64] => {
                let basic_types: Vec<_> = sig
                    .results()
                    .iter()
                    .map(|&ty| type_to_llvm(intrinsics, ty))
                    .collect::<Result<_, _>>()?;

                (
                    context.struct_type(&basic_types, false).fn_type(
                        param_types
                            .map(|v| v.map(Into::into))
                            .collect::<Result<Vec<BasicMetadataTypeEnum>, _>>()?
                            .as_slice(),
                        false,
                    ),
                    vmctx_attributes(0),
                )
            }
            [32, 32] if sig.results()[0] == Type::F32 && sig.results()[1] == Type::F32 => (
                intrinsics.f32_ty.vec_type(2).fn_type(
                    param_types
                        .map(|v| v.map(Into::into))
                        .collect::<Result<Vec<BasicMetadataTypeEnum>, _>>()?
                        .as_slice(),
                    false,
                ),
                vmctx_attributes(0),
            ),
            [32, 32] => (
                intrinsics.i64_ty.fn_type(
                    param_types
                        .map(|v| v.map(Into::into))
                        .collect::<Result<Vec<BasicMetadataTypeEnum>, _>>()?
                        .as_slice(),
                    false,
                ),
                vmctx_attributes(0),
            ),
            [32, 32, _] if sig.results()[0] == Type::F32 && sig.results()[1] == Type::F32 => (
                context
                    .struct_type(
                        &[
                            intrinsics.f32_ty.vec_type(2).as_basic_type_enum(),
                            type_to_llvm(intrinsics, sig.results()[2])?,
                        ],
                        false,
                    )
                    .fn_type(
                        param_types
                            .map(|v| v.map(Into::into))
                            .collect::<Result<Vec<BasicMetadataTypeEnum>, _>>()?
                            .as_slice(),
                        false,
                    ),
                vmctx_attributes(0),
            ),
            [32, 32, _] => (
                context
                    .struct_type(
                        &[
                            intrinsics.i64_ty.as_basic_type_enum(),
                            type_to_llvm(intrinsics, sig.results()[2])?,
                        ],
                        false,
                    )
                    .fn_type(
                        param_types
                            .map(|v| v.map(Into::into))
                            .collect::<Result<Vec<BasicMetadataTypeEnum>, _>>()?
                            .as_slice(),
                        false,
                    ),
                vmctx_attributes(0),
            ),
            [64, 32, 32] if sig.results()[1] == Type::F32 && sig.results()[2] == Type::F32 => (
                context
                    .struct_type(
                        &[
                            type_to_llvm(intrinsics, sig.results()[0])?,
                            intrinsics.f32_ty.vec_type(2).as_basic_type_enum(),
                        ],
                        false,
                    )
                    .fn_type(
                        param_types
                            .map(|v| v.map(Into::into))
                            .collect::<Result<Vec<BasicMetadataTypeEnum>, _>>()?
                            .as_slice(),
                        false,
                    ),
                vmctx_attributes(0),
            ),
            [64, 32, 32] => (
                context
                    .struct_type(
                        &[
                            type_to_llvm(intrinsics, sig.results()[0])?,
                            intrinsics.i64_ty.as_basic_type_enum(),
                        ],
                        false,
                    )
                    .fn_type(
                        param_types
                            .map(|v| v.map(Into::into))
                            .collect::<Result<Vec<BasicMetadataTypeEnum>, _>>()?
                            .as_slice(),
                        false,
                    ),
                vmctx_attributes(0),
            ),
            [32, 32, 32, 32] => (
                context
                    .struct_type(
                        &[
                            if sig.results()[0] == Type::F32 && sig.results()[1] == Type::F32 {
                                intrinsics.f32_ty.vec_type(2).as_basic_type_enum()
                            } else {
                                intrinsics.i64_ty.as_basic_type_enum()
                            },
                            if sig.results()[2] == Type::F32 && sig.results()[3] == Type::F32 {
                                intrinsics.f32_ty.vec_type(2).as_basic_type_enum()
                            } else {
                                intrinsics.i64_ty.as_basic_type_enum()
                            },
                        ],
                        false,
                    )
                    .fn_type(
                        param_types
                            .map(|v| v.map(Into::into))
                            .collect::<Result<Vec<BasicMetadataTypeEnum>, _>>()?
                            .as_slice(),
                        false,
                    ),
                vmctx_attributes(0),
            ),
            _ => {
                let basic_types: Vec<_> = sig
                    .results()
                    .iter()
                    .map(|&ty| type_to_llvm(intrinsics, ty))
                    .collect::<Result<_, _>>()?;

                let sret = context.struct_type(&basic_types, false);
                let sret_ptr = context.ptr_type(AddressSpace::default());

                let param_types =
                    std::iter::once(Ok(sret_ptr.as_basic_type_enum())).chain(param_types);

                let mut attributes = vec![(
                    context.create_type_attribute(
                        Attribute::get_named_enum_kind_id("sret"),
                        sret.as_any_type_enum(),
                    ),
                    AttributeLoc::Param(0),
                )];
                attributes.append(&mut vmctx_attributes(1));

                (
                    intrinsics.void_ty.fn_type(
                        param_types
                            .map(|v| v.map(Into::into))
                            .collect::<Result<Vec<BasicMetadataTypeEnum>, _>>()?
                            .as_slice(),
                        false,
                    ),
                    attributes,
                )
            }
        })
    }

    // Marshall wasm stack values into function parameters.
    fn args_to_call<'ctx>(
        &self,
        alloca_builder: &Builder<'ctx>,
        func_sig: &FuncSig,
        llvm_fn_ty: &FunctionType<'ctx>,
        ctx_ptr: PointerValue<'ctx>,
        values: &[BasicValueEnum<'ctx>],
        intrinsics: &Intrinsics<'ctx>,
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

        let values = std::iter::once(ctx_ptr.as_basic_value_enum()).chain(values.iter().copied());

        let ret = if let Some(sret) = sret {
            std::iter::once(sret.as_basic_value_enum())
                .chain(values)
                .collect()
        } else {
            values.collect()
        };

        Ok(ret)
    }

    // Given a CallSite, extract the returned values and return them in a Vec.
    fn rets_from_call<'ctx>(
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
                    Type::I32 => {
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

        if let Some(basic_value) = call_site.try_as_basic_value().left() {
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
                    .collect::<Vec<_>>();
                let func_sig_returns_bitwidths = func_sig
                    .results()
                    .iter()
                    .map(|ty| match ty {
                        Type::I32 | Type::F32 => 32,
                        Type::I64 | Type::F64 => 64,
                        Type::V128 => 128,
                        Type::ExternRef | Type::FuncRef | Type::ExceptionRef => 64, /* pointer */
                    })
                    .collect::<Vec<i32>>();

                let ret = match func_sig_returns_bitwidths.as_slice() {
                    [32, 64] | [64, 32] | [64, 64] => {
                        assert!(func_sig.results().len() == 2);
                        vec![rets[0], rets[1]]
                    }
                    [32, 32, _]
                        if rets[0].get_type()
                            == intrinsics.f32_ty.vec_type(2).as_basic_type_enum() =>
                    {
                        assert!(func_sig.results().len() == 3);
                        let (rets0, rets1) = extract_f32x2(rets[0].into_vector_value())?;
                        vec![rets0.into(), rets1.into(), rets[1]]
                    }
                    [32, 32, _] => {
                        assert!(func_sig.results().len() == 3);
                        let (low, high) = split_i64(rets[0].into_int_value())?;
                        let low = casted(low.into(), func_sig.results()[0])?;
                        let high = casted(high.into(), func_sig.results()[1])?;
                        vec![low, high, rets[1]]
                    }
                    [64, 32, 32]
                        if rets[1].get_type()
                            == intrinsics.f32_ty.vec_type(2).as_basic_type_enum() =>
                    {
                        assert!(func_sig.results().len() == 3);
                        let (rets1, rets2) = extract_f32x2(rets[1].into_vector_value())?;
                        vec![rets[0], rets1.into(), rets2.into()]
                    }
                    [64, 32, 32] => {
                        assert!(func_sig.results().len() == 3);
                        let (rets1, rets2) = split_i64(rets[1].into_int_value())?;
                        let rets1 = casted(rets1.into(), func_sig.results()[1])?;
                        let rets2 = casted(rets2.into(), func_sig.results()[2])?;
                        vec![rets[0], rets1, rets2]
                    }
                    [32, 32, 32, 32] => {
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
                        let low0 = casted(low0, func_sig.results()[0])?;
                        let high0 = casted(high0, func_sig.results()[1])?;
                        let low1 = casted(low1, func_sig.results()[2])?;
                        let high1 = casted(high1, func_sig.results()[3])?;
                        vec![low0, high0, low1, high1]
                    }
                    _ => unreachable!("expected an sret for this type"),
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
                    .right()
                    .unwrap()
                    .get_operand(0)
                    .unwrap()
                    .left()
                    .unwrap();
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

    fn is_sret(&self, func_sig: &FuncSig) -> Result<bool, CompileError> {
        let func_sig_returns_bitwidths = func_sig
            .results()
            .iter()
            .map(|ty| match ty {
                Type::I32 | Type::F32 => 32,
                Type::I64 | Type::F64 => 64,
                Type::V128 => 128,
                Type::ExternRef | Type::FuncRef | Type::ExceptionRef => 64, /* pointer */
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

    fn pack_values_for_register_return<'ctx>(
        &self,
        intrinsics: &Intrinsics<'ctx>,
        builder: &Builder<'ctx>,
        values: &[BasicValueEnum<'ctx>],
        func_type: &FunctionType<'ctx>,
    ) -> Result<BasicValueEnum<'ctx>, CompileError> {
        let is_32 = |value: BasicValueEnum| {
            (value.is_int_value() && value.into_int_value().get_type() == intrinsics.i32_ty)
                || (value.is_float_value()
                    && value.into_float_value().get_type() == intrinsics.f32_ty)
        };
        let is_64 = |value: BasicValueEnum| {
            (value.is_int_value() && value.into_int_value().get_type() == intrinsics.i64_ty)
                || (value.is_float_value()
                    && value.into_float_value().get_type() == intrinsics.f64_ty)
        };
        let is_f32 = |value: BasicValueEnum| {
            value.is_float_value() && value.into_float_value().get_type() == intrinsics.f32_ty
        };

        let pack_i32s = |low: BasicValueEnum<'ctx>, high: BasicValueEnum<'ctx>| {
            assert!(low.get_type() == intrinsics.i32_ty.as_basic_type_enum());
            assert!(high.get_type() == intrinsics.i32_ty.as_basic_type_enum());
            let (low, high) = (low.into_int_value(), high.into_int_value());
            let low = err!(builder.build_int_z_extend(low, intrinsics.i64_ty, ""));
            let high = err!(builder.build_int_z_extend(high, intrinsics.i64_ty, ""));
            let high =
                err!(builder.build_left_shift(high, intrinsics.i64_ty.const_int(32, false), ""));
            err_nt!(builder
                .build_or(low, high, "")
                .map(|v| v.as_basic_value_enum()))
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
            err_nt!(builder
                .build_insert_element(vec, second, intrinsics.i32_ty.const_int(1, false), "")
                .map(|v| v.as_basic_value_enum()))
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

        Ok(match *values {
            [one_value] => one_value,
            [v1, v2] if is_f32(v1) && is_f32(v2) => pack_f32s(v1, v2)?,
            [v1, v2] if is_32(v1) && is_32(v2) => {
                let v1 = err!(builder.build_bit_cast(v1, intrinsics.i32_ty, ""));
                let v2 = err!(builder.build_bit_cast(v2, intrinsics.i32_ty, ""));
                pack_i32s(v1, v2)?
            }
            [v1, v2] => {
                assert!(!(is_32(v1) && is_32(v2)));
                build_struct(
                    func_type.get_return_type().unwrap().into_struct_type(),
                    &[v1, v2],
                )
            }
            [v1, v2, v3] if is_f32(v1) && is_f32(v2) => build_struct(
                func_type.get_return_type().unwrap().into_struct_type(),
                &[pack_f32s(v1, v2)?, v3],
            ),
            [v1, v2, v3] if is_32(v1) && is_32(v2) => {
                let v1 = err!(builder.build_bit_cast(v1, intrinsics.i32_ty, ""));
                let v2 = err!(builder.build_bit_cast(v2, intrinsics.i32_ty, ""));
                build_struct(
                    func_type.get_return_type().unwrap().into_struct_type(),
                    &[pack_i32s(v1, v2)?, v3],
                )
            }
            [v1, v2, v3] if is_64(v1) && is_f32(v2) && is_f32(v3) => build_struct(
                func_type.get_return_type().unwrap().into_struct_type(),
                &[v1, pack_f32s(v2, v3)?],
            ),
            [v1, v2, v3] if is_64(v1) && is_32(v2) && is_32(v3) => {
                let v2 = err!(builder.build_bit_cast(v2, intrinsics.i32_ty, ""));
                let v3 = err!(builder.build_bit_cast(v3, intrinsics.i32_ty, ""));
                build_struct(
                    func_type.get_return_type().unwrap().into_struct_type(),
                    &[v1, pack_i32s(v2, v3)?],
                )
            }
            [v1, v2, v3, v4] if is_32(v1) && is_32(v2) && is_32(v3) && is_32(v4) => {
                let v1v2_pack = if is_f32(v1) && is_f32(v2) {
                    pack_f32s(v1, v2)?
                } else {
                    let v1 = err!(builder.build_bit_cast(v1, intrinsics.i32_ty, ""));
                    let v2 = err!(builder.build_bit_cast(v2, intrinsics.i32_ty, ""));
                    pack_i32s(v1, v2)?
                };
                let v3v4_pack = if is_f32(v3) && is_f32(v4) {
                    pack_f32s(v3, v4)?
                } else {
                    let v3 = err!(builder.build_bit_cast(v3, intrinsics.i32_ty, ""));
                    let v4 = err!(builder.build_bit_cast(v4, intrinsics.i32_ty, ""));
                    pack_i32s(v3, v4)?
                };
                build_struct(
                    func_type.get_return_type().unwrap().into_struct_type(),
                    &[v1v2_pack, v3v4_pack],
                )
            }
            _ => {
                unreachable!("called to perform register return on struct return or void function")
            }
        })
    }
}
