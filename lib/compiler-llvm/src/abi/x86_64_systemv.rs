use crate::abi::Abi;
use crate::error::{err, err_nt};
use crate::translator::intrinsics::{Intrinsics, type_to_llvm};
use inkwell::values::BasicValue;
use inkwell::{
    AddressSpace,
    attributes::{Attribute, AttributeLoc},
    builder::Builder,
    context::Context,
    types::{AnyType, BasicMetadataTypeEnum, BasicType, BasicTypeEnum, FunctionType, StructType},
    values::{BasicValueEnum, CallSiteValue, FloatValue, IntValue, VectorValue},
};
use itertools::Itertools;
use wasmer_types::{CompileError, FunctionType as FuncSig, Type};
use wasmer_vm::VMOffsets;

use std::convert::TryInto;

/// Describes how a list of values is returned by a ABI.
///
/// Every non-void variant retains the values it classified so signature
/// construction, packing, and unpacking all use the same ABI rules.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ReturnAbi {
    Void,
    Single(Type),
    Pair(Type, Type),
    PackedPair(Type, Type),
    PackedFirst(Type, Type, Type),
    PackedLast(Type, Type, Type),
    PackedQuads(Type, Type, Type, Type),
    Sret(Vec<Type>),
}

fn classify_x86_64(types: &[Type]) -> ReturnAbi {
    let widths = types.iter().map(wasm_type_bit_width).collect_vec();
    let values = types;

    match (values, widths.as_slice()) {
        ([], []) => ReturnAbi::Void,
        ([value], [_]) => ReturnAbi::Single(*value),
        ([first, second], [32, 64] | [64, 32] | [64, 64]) => ReturnAbi::Pair(*first, *second),
        ([first, second], [32, 32]) => ReturnAbi::PackedPair(*first, *second),
        ([first, second, third], [32, 32, 32 | 64]) => {
            ReturnAbi::PackedFirst(*first, *second, *third)
        }
        ([first, second, third], [64, 32, 32]) => ReturnAbi::PackedLast(*first, *second, *third),
        ([first, second, third, fourth], [32, 32, 32, 32]) => {
            ReturnAbi::PackedQuads(*first, *second, *third, *fourth)
        }
        _ => ReturnAbi::Sret(values.to_vec()),
    }
}

fn wasm_type_bit_width(ty: &Type) -> u32 {
    match ty {
        Type::I32 | Type::F32 | Type::ExceptionRef => 32,
        Type::I64 | Type::F64 | Type::ExternRef | Type::FuncRef => 64,
        Type::V128 => 128,
    }
}

/// Implementation of the [`Abi`] trait for the AMD64 SystemV ABI.
pub struct X86_64SystemV {}

impl Abi for X86_64SystemV {
    // Given a wasm function type, produce an llvm function declaration.
    fn func_type_to_llvm<'ctx>(
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

        let return_abi = classify_x86_64(sig.results());
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

            Ok((
                intrinsics
                    .void_ty
                    .fn_type(sret_param_llvm_types.as_slice(), false),
                attributes,
            ))
        } else {
            let function_type = match return_llvm_type {
                Some(return_type) => return_type.fn_type(param_llvm_types.as_slice(), false),
                None => intrinsics
                    .void_ty
                    .fn_type(param_llvm_types.as_slice(), false),
            };
            Ok((function_type, vmctx_attributes(0)))
        }
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
                let ret = match classify_x86_64(func_sig.results()) {
                    ReturnAbi::Pair(_, _) => {
                        assert!(func_sig.results().len() == 2);
                        vec![rets[0], rets[1]]
                    }
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

    fn pack_values_for_register_return<'ctx>(
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

        let return_abi = classify_x86_64(func_sig.results());

        Ok(match return_abi {
            ReturnAbi::Single(_) => values[0],
            ReturnAbi::PackedPair(Type::F32, Type::F32) => pack_f32s(values[0], values[1])?,
            ReturnAbi::PackedPair(_, _) => {
                let v1 = err!(builder.build_bit_cast(values[0], intrinsics.i32_ty, ""));
                let v2 = err!(builder.build_bit_cast(values[1], intrinsics.i32_ty, ""));
                pack_i32s(v1, v2)?
            }
            ReturnAbi::Pair(_, _) => build_struct(
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

#[cfg(test)]
mod tests {
    use super::{ReturnAbi, Type};
    use crate::abi::x86_64_systemv::classify_x86_64;

    #[test]
    fn classify_x86_64_return_type_abi() {
        assert_eq!(classify_x86_64(&[]), ReturnAbi::Void);
        assert_eq!(classify_x86_64(&[Type::I64]), ReturnAbi::Single(Type::I64));
        assert_eq!(
            classify_x86_64(&[Type::I32, Type::F64]),
            ReturnAbi::Pair(Type::I32, Type::F64)
        );
        assert_eq!(
            classify_x86_64(&[Type::I32, Type::F32]),
            ReturnAbi::PackedPair(Type::I32, Type::F32)
        );
        assert_eq!(
            classify_x86_64(&[Type::F32, Type::F32, Type::I64]),
            ReturnAbi::PackedFirst(Type::F32, Type::F32, Type::I64)
        );
        assert_eq!(
            classify_x86_64(&[Type::F64, Type::I32, Type::F32]),
            ReturnAbi::PackedLast(Type::F64, Type::I32, Type::F32)
        );
        assert_eq!(
            classify_x86_64(&[Type::I32, Type::F32, Type::F32, Type::I32]),
            ReturnAbi::PackedQuads(Type::I32, Type::F32, Type::F32, Type::I32)
        );
        assert_eq!(
            classify_x86_64(&[Type::V128, Type::I32]),
            ReturnAbi::Sret(vec![Type::V128, Type::I32])
        );
    }
}
