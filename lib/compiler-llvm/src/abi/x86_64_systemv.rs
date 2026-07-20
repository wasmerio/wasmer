use crate::abi::Abi;
use crate::error::{err, err_nt};
use crate::translator::intrinsics::{Intrinsics, type_to_llvm};
use inkwell::values::BasicValue;
use inkwell::{
    AddressSpace,
    attributes::{Attribute, AttributeLoc},
    builder::Builder,
    context::Context,
    types::{AnyType, BasicMetadataTypeEnum, BasicType, FunctionType, StructType},
    values::{BasicValueEnum, CallSiteValue, FloatValue, IntValue, VectorValue},
};
use itertools::Itertools;
use wasmer_types::{CompileError, FunctionType as FuncSig, Type};
use wasmer_vm::VMOffsets;

use std::convert::TryInto;

/// Describes how a list of values is returned by the AMD64 System V ABI.
///
/// Every non-void variant retains the values it classified so signature
/// construction, packing, and unpacking all use the same ABI rules.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ReturnAbi {
    Void,
    Single([Type; 1]),
    Pair([Type; 2]),
    PackedPair([Type; 2]),
    PackedFirst([Type; 3]),
    PackedLast([Type; 3]),
    PackedQuads([Type; 4]),
    Sret(Vec<Type>),
}

impl ReturnAbi {
    fn classify(types: &[Type]) -> Self {
        let widths = types.iter().map(wasm_type_bit_width).collect_vec();
        let values = types;

        match (values, widths.as_slice()) {
            ([], []) => Self::Void,
            ([value], [_]) => Self::Single([*value]),
            ([first, second], [32, 64] | [64, 32] | [64, 64]) => Self::Pair([*first, *second]),
            ([first, second], [32, 32]) => Self::PackedPair([*first, *second]),
            ([first, second, third], [32, 32, 32 | 64]) => {
                Self::PackedFirst([*first, *second, *third])
            }
            ([first, second, third], [64, 32, 32]) => Self::PackedLast([*first, *second, *third]),
            ([first, second, third, fourth], [32, 32, 32, 32]) => {
                Self::PackedQuads([*first, *second, *third, *fourth])
            }
            _ => Self::Sret(values.to_vec()),
        }
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
        let user_param_types = sig.params().iter().map(|&ty| type_to_llvm(intrinsics, ty));

        let mut param_types = vec![Ok(intrinsics.ptr_ty.as_basic_type_enum())];
        if include_m0_param {
            param_types.push(Ok(intrinsics.ptr_ty.as_basic_type_enum()));
        }

        let param_types = param_types.into_iter().chain(user_param_types);

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

        let return_abi = ReturnAbi::classify(sig.results());
        Ok(match return_abi {
            ReturnAbi::Void => (
                intrinsics.void_ty.fn_type(
                    param_types
                        .map(|v| v.map(Into::into))
                        .collect::<Result<Vec<BasicMetadataTypeEnum>, _>>()?
                        .as_slice(),
                    false,
                ),
                vmctx_attributes(0),
            ),
            ReturnAbi::Single([single_value]) => (
                type_to_llvm(intrinsics, single_value)?.fn_type(
                    param_types
                        .map(|v| v.map(Into::into))
                        .collect::<Result<Vec<BasicMetadataTypeEnum>, _>>()?
                        .as_slice(),
                    false,
                ),
                vmctx_attributes(0),
            ),
            ReturnAbi::Pair(types) => {
                let basic_types: Vec<_> = types
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
            ReturnAbi::PackedPair([Type::F32, Type::F32]) => (
                intrinsics.f32_ty.vec_type(2).fn_type(
                    param_types
                        .map(|v| v.map(Into::into))
                        .collect::<Result<Vec<BasicMetadataTypeEnum>, _>>()?
                        .as_slice(),
                    false,
                ),
                vmctx_attributes(0),
            ),
            ReturnAbi::PackedPair(_) => (
                intrinsics.i64_ty.fn_type(
                    param_types
                        .map(|v| v.map(Into::into))
                        .collect::<Result<Vec<BasicMetadataTypeEnum>, _>>()?
                        .as_slice(),
                    false,
                ),
                vmctx_attributes(0),
            ),
            ReturnAbi::PackedFirst([Type::F32, Type::F32, third]) => (
                context
                    .struct_type(
                        &[
                            intrinsics.f32_ty.vec_type(2).as_basic_type_enum(),
                            type_to_llvm(intrinsics, third)?,
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
            ReturnAbi::PackedFirst([_, _, third]) => (
                context
                    .struct_type(
                        &[
                            intrinsics.i64_ty.as_basic_type_enum(),
                            type_to_llvm(intrinsics, third)?,
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
            ReturnAbi::PackedLast([first, Type::F32, Type::F32]) => (
                context
                    .struct_type(
                        &[
                            type_to_llvm(intrinsics, first)?,
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
            ReturnAbi::PackedLast([first, _, _]) => (
                context
                    .struct_type(
                        &[
                            type_to_llvm(intrinsics, first)?,
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
            ReturnAbi::PackedQuads(types) => (
                context
                    .struct_type(
                        &[
                            if types[0] == Type::F32 && types[1] == Type::F32 {
                                intrinsics.f32_ty.vec_type(2).as_basic_type_enum()
                            } else {
                                intrinsics.i64_ty.as_basic_type_enum()
                            },
                            if types[2] == Type::F32 && types[3] == Type::F32 {
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
            ReturnAbi::Sret(types) => {
                let basic_types: Vec<_> = types
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
                    .collect::<Vec<_>>();
                let ret = match ReturnAbi::classify(func_sig.results()) {
                    ReturnAbi::Pair(_) => {
                        assert!(func_sig.results().len() == 2);
                        vec![rets[0], rets[1]]
                    }
                    ReturnAbi::PackedFirst([Type::F32, Type::F32, _]) => {
                        assert!(func_sig.results().len() == 3);
                        let (rets0, rets1) = extract_f32x2(rets[0].into_vector_value())?;
                        vec![rets0.into(), rets1.into(), rets[1]]
                    }
                    ReturnAbi::PackedFirst(types) => {
                        assert!(func_sig.results().len() == 3);
                        let (low, high) = split_i64(rets[0].into_int_value())?;
                        let low = casted(low.into(), types[0])?;
                        let high = casted(high.into(), types[1])?;
                        vec![low, high, rets[1]]
                    }
                    ReturnAbi::PackedLast([_, Type::F32, Type::F32]) => {
                        assert!(func_sig.results().len() == 3);
                        let (rets1, rets2) = extract_f32x2(rets[1].into_vector_value())?;
                        vec![rets[0], rets1.into(), rets2.into()]
                    }
                    ReturnAbi::PackedLast(types) => {
                        assert!(func_sig.results().len() == 3);
                        let (rets1, rets2) = split_i64(rets[1].into_int_value())?;
                        let rets1 = casted(rets1.into(), types[1])?;
                        let rets2 = casted(rets2.into(), types[2])?;
                        vec![rets[0], rets1, rets2]
                    }
                    ReturnAbi::PackedQuads(types) => {
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
                        let low0 = casted(low0, types[0])?;
                        let high0 = casted(high0, types[1])?;
                        let low1 = casted(low1, types[2])?;
                        let high1 = casted(high1, types[3])?;
                        vec![low0, high0, low1, high1]
                    }
                    ReturnAbi::Void
                    | ReturnAbi::Single(_)
                    | ReturnAbi::PackedPair(_)
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

    fn is_sret(&self, func_sig: &FuncSig) -> Result<bool, CompileError> {
        Ok(matches!(
            ReturnAbi::classify(func_sig.results()),
            ReturnAbi::Sret(_)
        ))
    }

    fn pack_values_for_register_return<'ctx>(
        &self,
        intrinsics: &Intrinsics<'ctx>,
        builder: &Builder<'ctx>,
        values: &[BasicValueEnum<'ctx>],
        func_type: &FunctionType<'ctx>,
    ) -> Result<BasicValueEnum<'ctx>, CompileError> {
        let wasm_type = |value: BasicValueEnum| {
            if value.is_int_value() {
                let ty = value.into_int_value().get_type();
                if ty == intrinsics.i32_ty {
                    Type::I32
                } else if ty == intrinsics.i64_ty {
                    Type::I64
                } else if ty == intrinsics.i128_ty {
                    Type::V128
                } else {
                    unreachable!("unsupported integer return type")
                }
            } else if value.is_float_value() {
                if value.into_float_value().get_type() == intrinsics.f32_ty {
                    Type::F32
                } else {
                    Type::F64
                }
            } else if value.is_pointer_value() {
                Type::ExternRef
            } else {
                unreachable!("unsupported return type")
            }
        };
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

        let value_types = values.iter().copied().map(wasm_type).collect::<Vec<_>>();
        let return_abi = ReturnAbi::classify(&value_types);

        Ok(match return_abi {
            ReturnAbi::Single(_) => values[0],
            ReturnAbi::PackedPair([Type::F32, Type::F32]) => pack_f32s(values[0], values[1])?,
            ReturnAbi::PackedPair(_) => {
                let v1 = err!(builder.build_bit_cast(values[0], intrinsics.i32_ty, ""));
                let v2 = err!(builder.build_bit_cast(values[1], intrinsics.i32_ty, ""));
                pack_i32s(v1, v2)?
            }
            ReturnAbi::Pair(_) => build_struct(
                func_type.get_return_type().unwrap().into_struct_type(),
                values,
            ),
            ReturnAbi::PackedFirst([Type::F32, Type::F32, _]) => build_struct(
                func_type.get_return_type().unwrap().into_struct_type(),
                &[pack_f32s(values[0], values[1])?, values[2]],
            ),
            ReturnAbi::PackedFirst(_) => {
                let v1 = err!(builder.build_bit_cast(values[0], intrinsics.i32_ty, ""));
                let v2 = err!(builder.build_bit_cast(values[1], intrinsics.i32_ty, ""));
                build_struct(
                    func_type.get_return_type().unwrap().into_struct_type(),
                    &[pack_i32s(v1, v2)?, values[2]],
                )
            }
            ReturnAbi::PackedLast([_, Type::F32, Type::F32]) => build_struct(
                func_type.get_return_type().unwrap().into_struct_type(),
                &[values[0], pack_f32s(values[1], values[2])?],
            ),
            ReturnAbi::PackedLast(_) => {
                let v2 = err!(builder.build_bit_cast(values[1], intrinsics.i32_ty, ""));
                let v3 = err!(builder.build_bit_cast(values[2], intrinsics.i32_ty, ""));
                build_struct(
                    func_type.get_return_type().unwrap().into_struct_type(),
                    &[values[0], pack_i32s(v2, v3)?],
                )
            }
            ReturnAbi::PackedQuads(types) => {
                let v1v2_pack = if types[0] == Type::F32 && types[1] == Type::F32 {
                    pack_f32s(values[0], values[1])?
                } else {
                    let v1 = err!(builder.build_bit_cast(values[0], intrinsics.i32_ty, ""));
                    let v2 = err!(builder.build_bit_cast(values[1], intrinsics.i32_ty, ""));
                    pack_i32s(v1, v2)?
                };
                let v3v4_pack = if types[2] == Type::F32 && types[3] == Type::F32 {
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

    fn classify(types: &[Type]) -> ReturnAbi {
        ReturnAbi::classify(types)
    }

    #[test]
    fn classifies_return_abi_and_preserves_types() {
        assert_eq!(classify(&[]), ReturnAbi::Void);
        assert_eq!(classify(&[Type::I64]), ReturnAbi::Single([Type::I64]));
        assert_eq!(
            classify(&[Type::I32, Type::F64]),
            ReturnAbi::Pair([Type::I32, Type::F64])
        );
        assert_eq!(
            classify(&[Type::I32, Type::F32]),
            ReturnAbi::PackedPair([Type::I32, Type::F32])
        );
        assert_eq!(
            classify(&[Type::F32, Type::F32, Type::I64]),
            ReturnAbi::PackedFirst([Type::F32, Type::F32, Type::I64])
        );
        assert_eq!(
            classify(&[Type::F64, Type::I32, Type::F32]),
            ReturnAbi::PackedLast([Type::F64, Type::I32, Type::F32])
        );
        assert_eq!(
            classify(&[Type::I32, Type::F32, Type::F32, Type::I32]),
            ReturnAbi::PackedQuads([Type::I32, Type::F32, Type::F32, Type::I32,])
        );
        assert_eq!(
            classify(&[Type::V128, Type::I32]),
            ReturnAbi::Sret(vec![Type::V128, Type::I32])
        );
    }
}
