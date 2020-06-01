// LLVM implements part of the ABI lowering internally, but also requires that
// the user pack and unpack values themselves sometimes. This can help the LLVM
// optimizer by exposing operations to the optimizer, but it requires that the
// frontend know exactly what IR to produce in order to get the right ABI.
//
// So far, this is an implementation of the SysV AMD64 ABI.

#![deny(
    dead_code,
    missing_docs,
)]

use crate::translator::intrinsics::{Intrinsics, type_to_llvm};
use inkwell::{
    attributes::{Attribute, AttributeLoc},
    builder::Builder,
    context::Context,
    types::{BasicType, FunctionType},
    values::{
        BasicValue, BasicValueEnum, CallSiteValue, FloatValue, FunctionValue, IntValue,
        PointerValue, VectorValue,
    },
    AddressSpace,
};
use wasm_common::{FunctionType as FuncSig, Type};

// Given a function definition, retrieve the parameter that is the vmctx pointer.
pub fn get_vmctx_ptr_param<'ctx>(func_value: &FunctionValue<'ctx>) -> PointerValue<'ctx> {
    func_value
        .get_nth_param(
            if func_value
                .get_enum_attribute(
                    AttributeLoc::Param(0),
                    Attribute::get_named_enum_kind_id("sret"),
                )
                .is_some()
            {
                1
            } else {
                0
            },
        )
        .unwrap()
        .into_pointer_value()
}

// Given a wasm function type, produce an llvm function declaration.
pub fn func_sig_to_llvm<'ctx>(
    context: &'ctx Context,
    intrinsics: &Intrinsics<'ctx>,
    sig: &FuncSig,
) -> (FunctionType<'ctx>, Vec<(Attribute, AttributeLoc)>) {
    let user_param_types = sig.params().iter().map(|&ty| type_to_llvm(intrinsics, ty));

    let param_types =
        std::iter::once(intrinsics.ctx_ptr_ty.as_basic_type_enum()).chain(user_param_types);

    let sig_returns_bitwidths = sig
        .results()
        .iter()
        .map(|ty| match ty {
            Type::I32 | Type::F32 => 32,
            Type::I64 | Type::F64 => 64,
            Type::V128 => 128,
            Type::AnyRef => unimplemented!("anyref in the llvm backend"),
            Type::FuncRef => unimplemented!("funcref in the llvm backend"),
        })
        .collect::<Vec<i32>>();

    match sig_returns_bitwidths.as_slice() {
        [] => (
            intrinsics
                .void_ty
                .fn_type(&param_types.collect::<Vec<_>>(), false),
            vec![],
        ),
        [_] => {
            let single_value = sig.results()[0];
            (
                type_to_llvm(intrinsics, single_value)
                    .fn_type(&param_types.collect::<Vec<_>>(), false),
                vec![],
            )
        }
        [32, 64] | [64, 32] | [64, 64] => {
            let basic_types: Vec<_> = sig
                .results()
                .iter()
                .map(|&ty| type_to_llvm(intrinsics, ty))
                .collect();

            (
                context
                    .struct_type(&basic_types, false)
                    .fn_type(&param_types.collect::<Vec<_>>(), false),
                vec![],
            )
        }
        [32, 32] if sig.results()[0] == Type::F32 && sig.results()[1] == Type::F32 => (
            intrinsics
                .f32_ty
                .vec_type(2)
                .fn_type(&param_types.collect::<Vec<_>>(), false),
            vec![],
        ),
        [32, 32] => (
            intrinsics
                .i64_ty
                .fn_type(&param_types.collect::<Vec<_>>(), false),
            vec![],
        ),
        [32, 32, _] if sig.results()[0] == Type::F32 && sig.results()[1] == Type::F32 => (
            context
                .struct_type(
                    &[
                        intrinsics.f32_ty.vec_type(2).as_basic_type_enum(),
                        type_to_llvm(intrinsics, sig.results()[2]),
                    ],
                    false,
                )
                .fn_type(&param_types.collect::<Vec<_>>(), false),
            vec![],
        ),
        [32, 32, _] => (
            context
                .struct_type(
                    &[
                        intrinsics.i64_ty.as_basic_type_enum(),
                        type_to_llvm(intrinsics, sig.results()[2]),
                    ],
                    false,
                )
                .fn_type(&param_types.collect::<Vec<_>>(), false),
            vec![],
        ),
        [64, 32, 32] if sig.results()[1] == Type::F32 && sig.results()[2] == Type::F32 => (
            context
                .struct_type(
                    &[
                        type_to_llvm(intrinsics, sig.results()[0]),
                        intrinsics.f32_ty.vec_type(2).as_basic_type_enum(),
                    ],
                    false,
                )
                .fn_type(&param_types.collect::<Vec<_>>(), false),
            vec![],
        ),
        [64, 32, 32] => (
            context
                .struct_type(
                    &[
                        type_to_llvm(intrinsics, sig.results()[0]),
                        intrinsics.i64_ty.as_basic_type_enum(),
                    ],
                    false,
                )
                .fn_type(&param_types.collect::<Vec<_>>(), false),
            vec![],
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
                .fn_type(&param_types.collect::<Vec<_>>(), false),
            vec![],
        ),
        _ => {
            let basic_types: Vec<_> = sig
                .results()
                .iter()
                .map(|&ty| type_to_llvm(intrinsics, ty))
                .collect();

            let sret = context
                .struct_type(&basic_types, false)
                .ptr_type(AddressSpace::Generic);

            let param_types = std::iter::once(sret.as_basic_type_enum()).chain(param_types);

            (
                intrinsics
                    .void_ty
                    .fn_type(&param_types.collect::<Vec<_>>(), false),
                vec![(
                    context.create_enum_attribute(Attribute::get_named_enum_kind_id("sret"), 0),
                    AttributeLoc::Param(0),
                )],
            )
        }
    }
}

// Marshall wasm stack values into function parameters.
pub fn args_to_call<'ctx>(
    alloca_builder: &Builder<'ctx>,
    func_sig: &FuncSig,
    ctx_ptr: PointerValue<'ctx>,
    llvm_fn_ty: &FunctionType<'ctx>,
    values: &[BasicValueEnum<'ctx>],
) -> Vec<BasicValueEnum<'ctx>> {
    // If it's an sret, allocate the return space.
    let sret = if llvm_fn_ty.get_return_type().is_none() && func_sig.results().len() > 1 {
        Some(
            alloca_builder.build_alloca(
                llvm_fn_ty.get_param_types()[0]
                    .into_pointer_type()
                    .get_element_type()
                    .into_struct_type(),
                "sret",
            ),
        )
    } else {
        None
    };

    let values = std::iter::once(ctx_ptr.as_basic_value_enum()).chain(values.iter().map(|x| *x));

    let values = if sret.is_some() {
        std::iter::once(sret.unwrap().as_basic_value_enum())
            .chain(values)
            .collect()
    } else {
        values.collect()
    };

    values
}

// Given a CallSite, extract the returned values and return them in a Vec.
pub fn rets_from_call<'ctx>(
    builder: &Builder<'ctx>,
    intrinsics: &Intrinsics<'ctx>,
    call_site: CallSiteValue<'ctx>,
    func_sig: &FuncSig,
) -> Vec<BasicValueEnum<'ctx>> {
    let split_i64 = |value: IntValue<'ctx>| -> (IntValue<'ctx>, IntValue<'ctx>) {
        assert!(value.get_type() == intrinsics.i64_ty);
        let low = builder.build_int_truncate(value, intrinsics.i32_ty, "");
        let lshr =
            builder.build_right_shift(value, intrinsics.i64_ty.const_int(32, false), false, "");
        let high = builder.build_int_truncate(lshr, intrinsics.i32_ty, "");
        (low, high)
    };

    let f32x2_ty = intrinsics.f32_ty.vec_type(2).as_basic_type_enum();
    let extract_f32x2 = |value: VectorValue<'ctx>| -> (FloatValue<'ctx>, FloatValue<'ctx>) {
        assert!(value.get_type() == f32x2_ty.into_vector_type());
        let ret0 = builder
            .build_extract_element(value, intrinsics.i32_ty.const_int(0, false), "")
            .into_float_value();
        let ret1 = builder
            .build_extract_element(value, intrinsics.i32_ty.const_int(1, false), "")
            .into_float_value();
        (ret0, ret1)
    };

    let casted = |value: BasicValueEnum<'ctx>, ty: Type| -> BasicValueEnum<'ctx> {
        match ty {
            Type::I32 => {
                assert!(
                    value.get_type() == intrinsics.i32_ty.as_basic_type_enum()
                        || value.get_type() == intrinsics.f32_ty.as_basic_type_enum()
                );
                builder.build_bitcast(value, intrinsics.i32_ty, "")
            }
            Type::F32 => {
                assert!(
                    value.get_type() == intrinsics.i32_ty.as_basic_type_enum()
                        || value.get_type() == intrinsics.f32_ty.as_basic_type_enum()
                );
                builder.build_bitcast(value, intrinsics.f32_ty, "")
            }
            Type::I64 => {
                assert!(
                    value.get_type() == intrinsics.i64_ty.as_basic_type_enum()
                        || value.get_type() == intrinsics.f64_ty.as_basic_type_enum()
                );
                builder.build_bitcast(value, intrinsics.i64_ty, "")
            }
            Type::F64 => {
                assert!(
                    value.get_type() == intrinsics.i64_ty.as_basic_type_enum()
                        || value.get_type() == intrinsics.f64_ty.as_basic_type_enum()
                );
                builder.build_bitcast(value, intrinsics.f64_ty, "")
            }
            Type::V128 => {
                assert!(value.get_type() == intrinsics.i128_ty.as_basic_type_enum());
                value
            }
            Type::AnyRef => unimplemented!("anyref in the llvm backend"),
            Type::FuncRef => unimplemented!("funcref in the llvm backend"),
        }
    };

    if let Some(basic_value) = call_site.try_as_basic_value().left() {
        if func_sig.results().len() > 1 {
            if basic_value.get_type() == intrinsics.i64_ty.as_basic_type_enum() {
                assert!(func_sig.results().len() == 2);
                let value = basic_value.into_int_value();
                let (low, high) = split_i64(value);
                let low = casted(low.into(), func_sig.results()[0]);
                let high = casted(high.into(), func_sig.results()[1]);
                return vec![low.into(), high.into()];
            }
            if basic_value.get_type() == f32x2_ty {
                assert!(func_sig.results().len() == 2);
                let (ret0, ret1) = extract_f32x2(basic_value.into_vector_value());
                return vec![ret0.into(), ret1.into()];
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
                    Type::AnyRef => unimplemented!("anyref in the llvm backend"),
                    Type::FuncRef => unimplemented!("funcref in the llvm backend"),
                })
                .collect::<Vec<i32>>();

            match func_sig_returns_bitwidths.as_slice() {
                [32, 64] | [64, 32] | [64, 64] => {
                    assert!(func_sig.results().len() == 2);
                    vec![rets[0].into(), rets[1].into()]
                }
                [32, 32, _]
                    if rets[0].get_type() == intrinsics.f32_ty.vec_type(2).as_basic_type_enum() =>
                {
                    assert!(func_sig.results().len() == 3);
                    let (rets0, rets1) = extract_f32x2(rets[0].into_vector_value());
                    vec![rets0.into(), rets1.into(), rets[1].into()]
                }
                [32, 32, _] => {
                    assert!(func_sig.results().len() == 3);
                    let (low, high) = split_i64(rets[0].into_int_value());
                    let low = casted(low.into(), func_sig.results()[0]);
                    let high = casted(high.into(), func_sig.results()[1]);
                    vec![low.into(), high.into(), rets[1].into()]
                }
                [64, 32, 32]
                    if rets[1].get_type() == intrinsics.f32_ty.vec_type(2).as_basic_type_enum() =>
                {
                    assert!(func_sig.results().len() == 3);
                    let (rets1, rets2) = extract_f32x2(rets[1].into_vector_value());
                    vec![rets[0].into(), rets1.into(), rets2.into()]
                }
                [64, 32, 32] => {
                    assert!(func_sig.results().len() == 3);
                    let (rets1, rets2) = split_i64(rets[1].into_int_value());
                    let rets1 = casted(rets1.into(), func_sig.results()[1]);
                    let rets2 = casted(rets2.into(), func_sig.results()[2]);
                    vec![rets[0].into(), rets1.into(), rets2.into()]
                }
                [32, 32, 32, 32] => {
                    assert!(func_sig.results().len() == 4);
                    let (low0, high0) = if rets[0].get_type()
                        == intrinsics.f32_ty.vec_type(2).as_basic_type_enum()
                    {
                        let (x, y) = extract_f32x2(rets[0].into_vector_value());
                        (x.into(), y.into())
                    } else {
                        let (x, y) = split_i64(rets[0].into_int_value());
                        (x.into(), y.into())
                    };
                    let (low1, high1) = if rets[1].get_type()
                        == intrinsics.f32_ty.vec_type(2).as_basic_type_enum()
                    {
                        let (x, y) = extract_f32x2(rets[1].into_vector_value());
                        (x.into(), y.into())
                    } else {
                        let (x, y) = split_i64(rets[1].into_int_value());
                        (x.into(), y.into())
                    };
                    let low0 = casted(low0, func_sig.results()[0]);
                    let high0 = casted(high0, func_sig.results()[1]);
                    let low1 = casted(low1, func_sig.results()[2]);
                    let high1 = casted(high1, func_sig.results()[3]);
                    vec![low0.into(), high0.into(), low1.into(), high1.into()]
                }
                _ => unreachable!("expected an sret for this type"),
            }
        } else {
            assert!(func_sig.results().len() == 1);
            vec![basic_value]
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
            let sret = call_site
                .try_as_basic_value()
                .right()
                .unwrap()
                .get_operand(0)
                .unwrap()
                .left()
                .unwrap()
                .into_pointer_value();
            let struct_value = builder.build_load(sret, "").into_struct_value();
            let mut rets: Vec<_> = Vec::new();
            for i in 0..struct_value.get_type().count_fields() {
                let value = builder.build_extract_value(struct_value, i, "").unwrap();
                rets.push(value);
            }
            assert!(func_sig.results().len() == rets.len());
            rets
        } else {
            assert!(func_sig.results().len() == 0);
            vec![]
        }
    }
}
