//! Code for dealing with [LLVM][llvm-intrinsics] and VM intrinsics.
//!
//! VM intrinsics are used to interact with the host VM.
//!
//! [llvm-intrinsics]: https://llvm.org/docs/LangRef.html#intrinsic-functions

use inkwell::{
    attributes::{Attribute, AttributeLoc},
    builder::Builder,
    context::Context,
    module::{Linkage, Module},
    types::{
        BasicType, BasicTypeEnum, FloatType, FunctionType, IntType, PointerType, StructType,
        VectorType, VoidType,
    },
    values::{
        BasicValue, BasicValueEnum, FloatValue, FunctionValue, InstructionValue, IntValue,
        PointerValue, VectorValue,
    },
    AddressSpace,
};
use std::collections::HashMap;
use wasm_common::entity::{EntityRef, PrimaryMap};
use wasm_common::{
    FunctionIndex, FunctionType as FuncType, GlobalIndex, MemoryIndex, Mutability, Pages,
    SignatureIndex, TableIndex, Type,
};
use wasmer_runtime::ModuleInfo as WasmerCompilerModule;
use wasmer_runtime::{MemoryPlan, MemoryStyle, TrapCode, VMOffsets};

pub fn type_to_llvm_ptr<'ctx>(intrinsics: &Intrinsics<'ctx>, ty: Type) -> PointerType<'ctx> {
    match ty {
        Type::I32 => intrinsics.i32_ptr_ty,
        Type::I64 => intrinsics.i64_ptr_ty,
        Type::F32 => intrinsics.f32_ptr_ty,
        Type::F64 => intrinsics.f64_ptr_ty,
        Type::V128 => intrinsics.i128_ptr_ty,
        Type::AnyRef => unimplemented!("anyref in the llvm backend"),
        Type::FuncRef => unimplemented!("funcref in the llvm backend"),
    }
}

/// Struct containing LLVM and VM intrinsics.
pub struct Intrinsics<'ctx> {
    pub ctlz_i32: FunctionValue<'ctx>,
    pub ctlz_i64: FunctionValue<'ctx>,

    pub cttz_i32: FunctionValue<'ctx>,
    pub cttz_i64: FunctionValue<'ctx>,

    pub ctpop_i32: FunctionValue<'ctx>,
    pub ctpop_i64: FunctionValue<'ctx>,

    pub sqrt_f32: FunctionValue<'ctx>,
    pub sqrt_f64: FunctionValue<'ctx>,
    pub sqrt_f32x4: FunctionValue<'ctx>,
    pub sqrt_f64x2: FunctionValue<'ctx>,

    pub ceil_f32: FunctionValue<'ctx>,
    pub ceil_f64: FunctionValue<'ctx>,

    pub floor_f32: FunctionValue<'ctx>,
    pub floor_f64: FunctionValue<'ctx>,

    pub trunc_f32: FunctionValue<'ctx>,
    pub trunc_f64: FunctionValue<'ctx>,

    pub nearbyint_f32: FunctionValue<'ctx>,
    pub nearbyint_f64: FunctionValue<'ctx>,

    pub fabs_f32: FunctionValue<'ctx>,
    pub fabs_f64: FunctionValue<'ctx>,
    pub fabs_f32x4: FunctionValue<'ctx>,
    pub fabs_f64x2: FunctionValue<'ctx>,

    pub copysign_f32: FunctionValue<'ctx>,
    pub copysign_f64: FunctionValue<'ctx>,

    pub sadd_sat_i8x16: FunctionValue<'ctx>,
    pub sadd_sat_i16x8: FunctionValue<'ctx>,
    pub uadd_sat_i8x16: FunctionValue<'ctx>,
    pub uadd_sat_i16x8: FunctionValue<'ctx>,

    pub ssub_sat_i8x16: FunctionValue<'ctx>,
    pub ssub_sat_i16x8: FunctionValue<'ctx>,
    pub usub_sat_i8x16: FunctionValue<'ctx>,
    pub usub_sat_i16x8: FunctionValue<'ctx>,

    pub expect_i1: FunctionValue<'ctx>,
    pub trap: FunctionValue<'ctx>,
    pub debug_trap: FunctionValue<'ctx>,

    pub personality: FunctionValue<'ctx>,

    pub void_ty: VoidType<'ctx>,
    pub i1_ty: IntType<'ctx>,
    pub i8_ty: IntType<'ctx>,
    pub i16_ty: IntType<'ctx>,
    pub i32_ty: IntType<'ctx>,
    pub i64_ty: IntType<'ctx>,
    pub i128_ty: IntType<'ctx>,
    pub f32_ty: FloatType<'ctx>,
    pub f64_ty: FloatType<'ctx>,

    pub i1x128_ty: VectorType<'ctx>,
    pub i8x16_ty: VectorType<'ctx>,
    pub i16x8_ty: VectorType<'ctx>,
    pub i32x4_ty: VectorType<'ctx>,
    pub i64x2_ty: VectorType<'ctx>,
    pub f32x4_ty: VectorType<'ctx>,
    pub f64x2_ty: VectorType<'ctx>,

    pub i8_ptr_ty: PointerType<'ctx>,
    pub i16_ptr_ty: PointerType<'ctx>,
    pub i32_ptr_ty: PointerType<'ctx>,
    pub i64_ptr_ty: PointerType<'ctx>,
    pub i128_ptr_ty: PointerType<'ctx>,
    pub f32_ptr_ty: PointerType<'ctx>,
    pub f64_ptr_ty: PointerType<'ctx>,

    pub anyfunc_ty: StructType<'ctx>,

    pub i1_zero: IntValue<'ctx>,
    pub i8_zero: IntValue<'ctx>,
    pub i32_zero: IntValue<'ctx>,
    pub i64_zero: IntValue<'ctx>,
    pub i128_zero: IntValue<'ctx>,
    pub f32_zero: FloatValue<'ctx>,
    pub f64_zero: FloatValue<'ctx>,
    pub f32x4_zero: VectorValue<'ctx>,
    pub f64x2_zero: VectorValue<'ctx>,

    pub trap_unreachable: BasicValueEnum<'ctx>,
    pub trap_call_indirect_null: BasicValueEnum<'ctx>,
    pub trap_call_indirect_sig: BasicValueEnum<'ctx>,
    pub trap_memory_oob: BasicValueEnum<'ctx>,
    pub trap_illegal_arithmetic: BasicValueEnum<'ctx>,
    pub trap_integer_division_by_zero: BasicValueEnum<'ctx>,
    pub trap_bad_conversion_to_integer: BasicValueEnum<'ctx>,
    pub trap_unaligned_atomic: BasicValueEnum<'ctx>,
    pub trap_table_access_oob: BasicValueEnum<'ctx>,

    // VM intrinsics.
    pub memory_grow_dynamic_local: FunctionValue<'ctx>,
    pub memory_grow_static_local: FunctionValue<'ctx>,
    pub memory_grow_shared_local: FunctionValue<'ctx>,
    pub memory_grow_dynamic_import: FunctionValue<'ctx>,
    pub memory_grow_static_import: FunctionValue<'ctx>,
    pub memory_grow_shared_import: FunctionValue<'ctx>,

    pub memory_size_dynamic_local: FunctionValue<'ctx>,
    pub memory_size_static_local: FunctionValue<'ctx>,
    pub memory_size_shared_local: FunctionValue<'ctx>,
    pub memory_size_dynamic_import: FunctionValue<'ctx>,
    pub memory_size_static_import: FunctionValue<'ctx>,
    pub memory_size_shared_import: FunctionValue<'ctx>,

    pub throw_trap: FunctionValue<'ctx>,
    pub throw_breakpoint: FunctionValue<'ctx>,

    pub experimental_stackmap: FunctionValue<'ctx>,

    pub vmfunction_import_ptr_ty: PointerType<'ctx>,
    pub vmfunction_import_body_element: u32,
    pub vmfunction_import_vmctx_element: u32,

    pub vmmemory_definition_ptr_ty: PointerType<'ctx>,
    pub vmmemory_definition_base_element: u32,
    pub vmmemory_definition_current_length_element: u32,

    pub memory32_grow_ptr_ty: PointerType<'ctx>,
    pub imported_memory32_grow_ptr_ty: PointerType<'ctx>,
    pub memory32_size_ptr_ty: PointerType<'ctx>,
    pub imported_memory32_size_ptr_ty: PointerType<'ctx>,

    pub ctx_ptr_ty: PointerType<'ctx>,
}

impl<'ctx> Intrinsics<'ctx> {
    /// Create an [`Intrinsics`] for the given [`Context`].
    pub fn declare(module: &Module<'ctx>, context: &'ctx Context) -> Self {
        let void_ty = context.void_type();
        let i1_ty = context.bool_type();
        let i8_ty = context.i8_type();
        let i16_ty = context.i16_type();
        let i32_ty = context.i32_type();
        let i64_ty = context.i64_type();
        let i128_ty = context.i128_type();
        let f32_ty = context.f32_type();
        let f64_ty = context.f64_type();

        let i1x128_ty = i1_ty.vec_type(128);
        let i8x16_ty = i8_ty.vec_type(16);
        let i16x8_ty = i16_ty.vec_type(8);
        let i32x4_ty = i32_ty.vec_type(4);
        let i64x2_ty = i64_ty.vec_type(2);
        let f32x4_ty = f32_ty.vec_type(4);
        let f64x2_ty = f64_ty.vec_type(2);

        let i8_ptr_ty = i8_ty.ptr_type(AddressSpace::Generic);
        let i16_ptr_ty = i16_ty.ptr_type(AddressSpace::Generic);
        let i32_ptr_ty = i32_ty.ptr_type(AddressSpace::Generic);
        let i64_ptr_ty = i64_ty.ptr_type(AddressSpace::Generic);
        let i128_ptr_ty = i128_ty.ptr_type(AddressSpace::Generic);
        let f32_ptr_ty = f32_ty.ptr_type(AddressSpace::Generic);
        let f64_ptr_ty = f64_ty.ptr_type(AddressSpace::Generic);

        let i1_zero = i1_ty.const_int(0, false);
        let i8_zero = i8_ty.const_int(0, false);
        let i32_zero = i32_ty.const_int(0, false);
        let i64_zero = i64_ty.const_int(0, false);
        let i128_zero = i128_ty.const_int(0, false);
        let f32_zero = f32_ty.const_float(0.0);
        let f64_zero = f64_ty.const_float(0.0);
        let f32x4_zero = f32x4_ty.const_zero();
        let f64x2_zero = f64x2_ty.const_zero();

        let i1_ty_basic = i1_ty.as_basic_type_enum();
        let i32_ty_basic = i32_ty.as_basic_type_enum();
        let i64_ty_basic = i64_ty.as_basic_type_enum();
        let f32_ty_basic = f32_ty.as_basic_type_enum();
        let f64_ty_basic = f64_ty.as_basic_type_enum();
        let i8x16_ty_basic = i8x16_ty.as_basic_type_enum();
        let i16x8_ty_basic = i16x8_ty.as_basic_type_enum();
        let f32x4_ty_basic = f32x4_ty.as_basic_type_enum();
        let f64x2_ty_basic = f64x2_ty.as_basic_type_enum();
        let i8_ptr_ty_basic = i8_ptr_ty.as_basic_type_enum();
        let i64_ptr_ty_basic = i64_ptr_ty.as_basic_type_enum();

        let ctx_ty = i8_ty;
        let ctx_ptr_ty = ctx_ty.ptr_type(AddressSpace::Generic);

        let local_memory_ty =
            context.struct_type(&[i8_ptr_ty_basic, i64_ty_basic, i8_ptr_ty_basic], false);
        let local_table_ty = local_memory_ty;
        let local_global_ty = i64_ty;
        let func_ctx_ty =
            context.struct_type(&[ctx_ptr_ty.as_basic_type_enum(), i8_ptr_ty_basic], false);
        let func_ctx_ptr_ty = func_ctx_ty.ptr_type(AddressSpace::Generic);
        let imported_func_ty = context.struct_type(
            &[i8_ptr_ty_basic, func_ctx_ptr_ty.as_basic_type_enum()],
            false,
        );
        let sigindex_ty = i32_ty;
        let rt_intrinsics_ty = i8_ty;
        let stack_lower_bound_ty = i8_ty;
        let memory_base_ty = i8_ty;
        let memory_bound_ty = i8_ty;
        let internals_ty = i64_ty;
        let interrupt_signal_mem_ty = i8_ty;
        let local_function_ty = i8_ptr_ty;

        let anyfunc_ty = context.struct_type(
            &[
                i8_ptr_ty_basic,
                sigindex_ty.as_basic_type_enum(),
                ctx_ptr_ty.as_basic_type_enum(),
            ],
            false,
        );

        let ret_i8x16_take_i8x16_i8x16 = i8x16_ty.fn_type(&[i8x16_ty_basic, i8x16_ty_basic], false);
        let ret_i16x8_take_i16x8_i16x8 = i16x8_ty.fn_type(&[i16x8_ty_basic, i16x8_ty_basic], false);

        let ret_i32_take_i32_i1 = i32_ty.fn_type(&[i32_ty_basic, i1_ty_basic], false);
        let ret_i64_take_i64_i1 = i64_ty.fn_type(&[i64_ty_basic, i1_ty_basic], false);

        let ret_i32_take_i32 = i32_ty.fn_type(&[i32_ty_basic], false);
        let ret_i64_take_i64 = i64_ty.fn_type(&[i64_ty_basic], false);

        let ret_f32_take_f32 = f32_ty.fn_type(&[f32_ty_basic], false);
        let ret_f64_take_f64 = f64_ty.fn_type(&[f64_ty_basic], false);
        let ret_f32x4_take_f32x4 = f32x4_ty.fn_type(&[f32x4_ty_basic], false);
        let ret_f64x2_take_f64x2 = f64x2_ty.fn_type(&[f64x2_ty_basic], false);

        let ret_f32_take_f32_f32 = f32_ty.fn_type(&[f32_ty_basic, f32_ty_basic], false);
        let ret_f64_take_f64_f64 = f64_ty.fn_type(&[f64_ty_basic, f64_ty_basic], false);

        let ret_i32_take_ctx_i32_i32 = i32_ty.fn_type(
            &[ctx_ptr_ty.as_basic_type_enum(), i32_ty_basic, i32_ty_basic],
            false,
        );
        let ret_i32_take_ctx_i32 =
            i32_ty.fn_type(&[ctx_ptr_ty.as_basic_type_enum(), i32_ty_basic], false);

        let ret_i1_take_i1_i1 = i1_ty.fn_type(&[i1_ty_basic, i1_ty_basic], false);
        let intrinsics = Self {
            ctlz_i32: module.add_function("llvm.ctlz.i32", ret_i32_take_i32_i1, None),
            ctlz_i64: module.add_function("llvm.ctlz.i64", ret_i64_take_i64_i1, None),

            cttz_i32: module.add_function("llvm.cttz.i32", ret_i32_take_i32_i1, None),
            cttz_i64: module.add_function("llvm.cttz.i64", ret_i64_take_i64_i1, None),

            ctpop_i32: module.add_function("llvm.ctpop.i32", ret_i32_take_i32, None),
            ctpop_i64: module.add_function("llvm.ctpop.i64", ret_i64_take_i64, None),

            sqrt_f32: module.add_function("llvm.sqrt.f32", ret_f32_take_f32, None),
            sqrt_f64: module.add_function("llvm.sqrt.f64", ret_f64_take_f64, None),
            sqrt_f32x4: module.add_function("llvm.sqrt.v4f32", ret_f32x4_take_f32x4, None),
            sqrt_f64x2: module.add_function("llvm.sqrt.v2f64", ret_f64x2_take_f64x2, None),

            ceil_f32: module.add_function("llvm.ceil.f32", ret_f32_take_f32, None),
            ceil_f64: module.add_function("llvm.ceil.f64", ret_f64_take_f64, None),

            floor_f32: module.add_function("llvm.floor.f32", ret_f32_take_f32, None),
            floor_f64: module.add_function("llvm.floor.f64", ret_f64_take_f64, None),

            trunc_f32: module.add_function("llvm.trunc.f32", ret_f32_take_f32, None),
            trunc_f64: module.add_function("llvm.trunc.f64", ret_f64_take_f64, None),

            nearbyint_f32: module.add_function("llvm.nearbyint.f32", ret_f32_take_f32, None),
            nearbyint_f64: module.add_function("llvm.nearbyint.f64", ret_f64_take_f64, None),

            fabs_f32: module.add_function("llvm.fabs.f32", ret_f32_take_f32, None),
            fabs_f64: module.add_function("llvm.fabs.f64", ret_f64_take_f64, None),
            fabs_f32x4: module.add_function("llvm.fabs.v4f32", ret_f32x4_take_f32x4, None),
            fabs_f64x2: module.add_function("llvm.fabs.v2f64", ret_f64x2_take_f64x2, None),

            copysign_f32: module.add_function("llvm.copysign.f32", ret_f32_take_f32_f32, None),
            copysign_f64: module.add_function("llvm.copysign.f64", ret_f64_take_f64_f64, None),

            sadd_sat_i8x16: module.add_function(
                "llvm.sadd.sat.v16i8",
                ret_i8x16_take_i8x16_i8x16,
                None,
            ),
            sadd_sat_i16x8: module.add_function(
                "llvm.sadd.sat.v8i16",
                ret_i16x8_take_i16x8_i16x8,
                None,
            ),
            uadd_sat_i8x16: module.add_function(
                "llvm.uadd.sat.v16i8",
                ret_i8x16_take_i8x16_i8x16,
                None,
            ),
            uadd_sat_i16x8: module.add_function(
                "llvm.uadd.sat.v8i16",
                ret_i16x8_take_i16x8_i16x8,
                None,
            ),

            ssub_sat_i8x16: module.add_function(
                "llvm.ssub.sat.v16i8",
                ret_i8x16_take_i8x16_i8x16,
                None,
            ),
            ssub_sat_i16x8: module.add_function(
                "llvm.ssub.sat.v8i16",
                ret_i16x8_take_i16x8_i16x8,
                None,
            ),
            usub_sat_i8x16: module.add_function(
                "llvm.usub.sat.v16i8",
                ret_i8x16_take_i8x16_i8x16,
                None,
            ),
            usub_sat_i16x8: module.add_function(
                "llvm.usub.sat.v8i16",
                ret_i16x8_take_i16x8_i16x8,
                None,
            ),

            expect_i1: module.add_function("llvm.expect.i1", ret_i1_take_i1_i1, None),
            trap: module.add_function("llvm.trap", void_ty.fn_type(&[], false), None),
            debug_trap: module.add_function("llvm.debugtrap", void_ty.fn_type(&[], false), None),
            personality: module.add_function(
                "__gxx_personality_v0",
                i32_ty.fn_type(&[], false),
                Some(Linkage::External),
            ),

            void_ty,
            i1_ty,
            i8_ty,
            i16_ty,
            i32_ty,
            i64_ty,
            i128_ty,
            f32_ty,
            f64_ty,

            i1x128_ty,
            i8x16_ty,
            i16x8_ty,
            i32x4_ty,
            i64x2_ty,
            f32x4_ty,
            f64x2_ty,

            i8_ptr_ty,
            i16_ptr_ty,
            i32_ptr_ty,
            i64_ptr_ty,
            i128_ptr_ty,
            f32_ptr_ty,
            f64_ptr_ty,

            anyfunc_ty,

            i1_zero,
            i8_zero,
            i32_zero,
            i64_zero,
            i128_zero,
            f32_zero,
            f64_zero,
            f32x4_zero,
            f64x2_zero,

            trap_unreachable: i32_ty
                .const_int(TrapCode::UnreachableCodeReached as _, false)
                .as_basic_value_enum(),
            trap_call_indirect_null: i32_ty
                .const_int(TrapCode::IndirectCallToNull as _, false)
                .as_basic_value_enum(),
            trap_call_indirect_sig: i32_ty
                .const_int(TrapCode::BadSignature as _, false)
                .as_basic_value_enum(),
            trap_memory_oob: i32_ty
                .const_int(TrapCode::OutOfBounds as _, false)
                .as_basic_value_enum(),
            trap_illegal_arithmetic: i32_ty
                .const_int(TrapCode::IntegerOverflow as _, false)
                .as_basic_value_enum(),
            trap_integer_division_by_zero: i32_ty
                .const_int(TrapCode::IntegerDivisionByZero as _, false)
                .as_basic_value_enum(),
            trap_bad_conversion_to_integer: i32_ty
                .const_int(TrapCode::BadConversionToInteger as _, false)
                .as_basic_value_enum(),
            trap_unaligned_atomic: i32_ty
                .const_int(TrapCode::UnalignedAtomic as _, false)
                .as_basic_value_enum(),
            trap_table_access_oob: i32_ty
                .const_int(TrapCode::TableAccessOutOfBounds as _, false)
                .as_basic_value_enum(),

            // VM intrinsics.
            memory_grow_dynamic_local: module.add_function(
                "vm.memory.grow.dynamic.local",
                ret_i32_take_ctx_i32_i32,
                None,
            ),
            memory_grow_static_local: module.add_function(
                "vm.memory.grow.static.local",
                ret_i32_take_ctx_i32_i32,
                None,
            ),
            memory_grow_shared_local: module.add_function(
                "vm.memory.grow.shared.local",
                ret_i32_take_ctx_i32_i32,
                None,
            ),
            memory_grow_dynamic_import: module.add_function(
                "vm.memory.grow.dynamic.import",
                ret_i32_take_ctx_i32_i32,
                None,
            ),
            memory_grow_static_import: module.add_function(
                "vm.memory.grow.static.import",
                ret_i32_take_ctx_i32_i32,
                None,
            ),
            memory_grow_shared_import: module.add_function(
                "vm.memory.grow.shared.import",
                ret_i32_take_ctx_i32_i32,
                None,
            ),

            memory_size_dynamic_local: module.add_function(
                "vm.memory.size.dynamic.local",
                ret_i32_take_ctx_i32,
                None,
            ),
            memory_size_static_local: module.add_function(
                "vm.memory.size.static.local",
                ret_i32_take_ctx_i32,
                None,
            ),
            memory_size_shared_local: module.add_function(
                "vm.memory.size.shared.local",
                ret_i32_take_ctx_i32,
                None,
            ),
            memory_size_dynamic_import: module.add_function(
                "vm.memory.size.dynamic.import",
                ret_i32_take_ctx_i32,
                None,
            ),
            memory_size_static_import: module.add_function(
                "vm.memory.size.static.import",
                ret_i32_take_ctx_i32,
                None,
            ),
            memory_size_shared_import: module.add_function(
                "vm.memory.size.shared.import",
                ret_i32_take_ctx_i32,
                None,
            ),
            throw_trap: module.add_function(
                "vm.exception.trap",
                void_ty.fn_type(&[i32_ty_basic], false),
                None,
            ),
            experimental_stackmap: module.add_function(
                "llvm.experimental.stackmap",
                void_ty.fn_type(
                    &[
                        i64_ty_basic, /* id */
                        i32_ty_basic, /* numShadowBytes */
                    ],
                    true,
                ),
                None,
            ),
            throw_breakpoint: module.add_function(
                "vm.breakpoint",
                void_ty.fn_type(&[i64_ty_basic], false),
                None,
            ),

            vmfunction_import_ptr_ty: context
                .struct_type(&[i8_ptr_ty_basic, i8_ptr_ty_basic], false)
                .ptr_type(AddressSpace::Generic),
            vmfunction_import_body_element: 0,
            vmfunction_import_vmctx_element: 1,

            // TODO: this i64 is actually a rust usize
            vmmemory_definition_ptr_ty: context
                .struct_type(&[i8_ptr_ty_basic, i64_ptr_ty_basic], false)
                .ptr_type(AddressSpace::Generic),
            vmmemory_definition_base_element: 0,
            vmmemory_definition_current_length_element: 1,

            memory32_grow_ptr_ty: i32_ty
                .fn_type(
                    &[ctx_ptr_ty.as_basic_type_enum(), i32_ty_basic, i32_ty_basic],
                    false,
                )
                .ptr_type(AddressSpace::Generic),
            imported_memory32_grow_ptr_ty: i32_ty
                .fn_type(
                    &[ctx_ptr_ty.as_basic_type_enum(), i32_ty_basic, i32_ty_basic],
                    false,
                )
                .ptr_type(AddressSpace::Generic),
            memory32_size_ptr_ty: i32_ty
                .fn_type(&[ctx_ptr_ty.as_basic_type_enum(), i32_ty_basic], false)
                .ptr_type(AddressSpace::Generic),
            imported_memory32_size_ptr_ty: i32_ty
                .fn_type(&[ctx_ptr_ty.as_basic_type_enum(), i32_ty_basic], false)
                .ptr_type(AddressSpace::Generic),

            ctx_ptr_ty,
        };

        // TODO: mark vmctx args as nofree, align 16, dereferenceable(?)

        let readonly =
            context.create_enum_attribute(Attribute::get_named_enum_kind_id("readonly"), 0);
        intrinsics
            .memory_size_dynamic_local
            .add_attribute(AttributeLoc::Function, readonly);
        intrinsics
            .memory_size_static_local
            .add_attribute(AttributeLoc::Function, readonly);
        intrinsics
            .memory_size_shared_local
            .add_attribute(AttributeLoc::Function, readonly);
        intrinsics
            .memory_size_dynamic_import
            .add_attribute(AttributeLoc::Function, readonly);
        intrinsics
            .memory_size_static_import
            .add_attribute(AttributeLoc::Function, readonly);
        intrinsics
            .memory_size_shared_import
            .add_attribute(AttributeLoc::Function, readonly);

        let noreturn =
            context.create_enum_attribute(Attribute::get_named_enum_kind_id("noreturn"), 0);
        intrinsics
            .throw_trap
            .add_attribute(AttributeLoc::Function, noreturn);
        intrinsics
            .throw_breakpoint
            .add_attribute(AttributeLoc::Function, noreturn);

        intrinsics
    }
}

#[derive(Clone, Copy)]
pub enum MemoryCache<'ctx> {
    /// The memory moves around.
    Dynamic {
        ptr_to_base_ptr: PointerValue<'ctx>,
        current_length_ptr: PointerValue<'ctx>,
    },
    /// The memory is always in the same place.
    Static { base_ptr: PointerValue<'ctx> },
}

struct TableCache<'ctx> {
    ptr_to_base_ptr: PointerValue<'ctx>,
    ptr_to_bounds: PointerValue<'ctx>,
}

#[derive(Clone, Copy)]
pub enum GlobalCache<'ctx> {
    Mut { ptr_to_value: PointerValue<'ctx> },
    Const { value: BasicValueEnum<'ctx> },
}

struct ImportedFuncCache<'ctx> {
    func_ptr: PointerValue<'ctx>,
    ctx_ptr: PointerValue<'ctx>,
}

pub struct CtxType<'ctx, 'a> {
    ctx_ptr_value: PointerValue<'ctx>,

    wasm_module: &'a WasmerCompilerModule,
    cache_builder: &'a Builder<'ctx>,

    cached_signal_mem: Option<PointerValue<'ctx>>,

    cached_memories: HashMap<MemoryIndex, MemoryCache<'ctx>>,
    cached_tables: HashMap<TableIndex, TableCache<'ctx>>,
    cached_sigindices: HashMap<SignatureIndex, IntValue<'ctx>>,
    cached_globals: HashMap<GlobalIndex, GlobalCache<'ctx>>,
    cached_imported_functions: HashMap<FunctionIndex, ImportedFuncCache<'ctx>>,

    offsets: VMOffsets,
}

impl<'ctx, 'a> CtxType<'ctx, 'a> {
    pub fn new(
        wasm_module: &'a WasmerCompilerModule,
        func_value: &FunctionValue<'ctx>,
        cache_builder: &'a Builder<'ctx>,
    ) -> CtxType<'ctx, 'a> {
        CtxType {
            ctx_ptr_value: func_value.get_nth_param(0).unwrap().into_pointer_value(),

            wasm_module,
            cache_builder,

            cached_signal_mem: None,

            cached_memories: HashMap::new(),
            cached_tables: HashMap::new(),
            cached_sigindices: HashMap::new(),
            cached_globals: HashMap::new(),
            cached_imported_functions: HashMap::new(),

            // TODO: pointer width
            offsets: VMOffsets::new(8, &wasm_module),
        }
    }

    pub fn basic(&self) -> BasicValueEnum<'ctx> {
        self.ctx_ptr_value.as_basic_value_enum()
    }

    pub fn memory(
        &mut self,
        index: MemoryIndex,
        intrinsics: &Intrinsics<'ctx>,
        module: &Module<'ctx>,
        memory_plans: &PrimaryMap<MemoryIndex, MemoryPlan>,
    ) -> MemoryCache<'ctx> {
        let (cached_memories, wasm_module, ctx_ptr_value, cache_builder, offsets) = (
            &mut self.cached_memories,
            self.wasm_module,
            self.ctx_ptr_value,
            &self.cache_builder,
            &self.offsets,
        );
        let memory_plan = &memory_plans[index];
        *cached_memories.entry(index).or_insert_with(|| {
            let memory_definition_ptr =
                if let Some(local_memory_index) = wasm_module.local_memory_index(index) {
                    let offset = offsets.vmctx_vmmemory_definition(local_memory_index);
                    let offset = intrinsics.i32_ty.const_int(offset.into(), false);
                    unsafe { cache_builder.build_gep(ctx_ptr_value, &[offset], "") }
                } else {
                    let offset = offsets.vmctx_vmmemory_import(index);
                    let offset = intrinsics.i32_ty.const_int(offset.into(), false);
                    let memory_definition_ptr_ptr =
                        unsafe { cache_builder.build_gep(ctx_ptr_value, &[offset], "") };
                    let memory_definition_ptr_ptr = cache_builder
                        .build_bitcast(
                            memory_definition_ptr_ptr,
                            intrinsics.i8_ptr_ty.ptr_type(AddressSpace::Generic),
                            "",
                        )
                        .into_pointer_value();
                    cache_builder
                        .build_load(memory_definition_ptr_ptr, "")
                        .into_pointer_value()
                    // TODO: tbaa
                };
            let memory_definition_ptr = cache_builder
                .build_bitcast(
                    memory_definition_ptr,
                    intrinsics.vmmemory_definition_ptr_ty,
                    "",
                )
                .into_pointer_value();
            let base_ptr = cache_builder
                .build_struct_gep(
                    memory_definition_ptr,
                    intrinsics.vmmemory_definition_base_element,
                    "",
                )
                .unwrap();
            if memory_plan.style == MemoryStyle::Dynamic {
                let current_length_ptr = cache_builder
                    .build_struct_gep(
                        memory_definition_ptr,
                        intrinsics.vmmemory_definition_current_length_element,
                        "",
                    )
                    .unwrap();
                MemoryCache::Dynamic {
                    ptr_to_base_ptr: base_ptr,
                    current_length_ptr,
                }
            } else {
                let base_ptr = cache_builder.build_load(base_ptr, "").into_pointer_value();
                // TODO: tbaa
                MemoryCache::Static { base_ptr }
            }
        })
    }

    fn table_prepare(
        &mut self,
        table_index: TableIndex,
        intrinsics: &Intrinsics<'ctx>,
        module: &Module<'ctx>,
    ) -> (PointerValue<'ctx>, PointerValue<'ctx>) {
        let (cached_tables, wasm_module, ctx_ptr_value, cache_builder, offsets) = (
            &mut self.cached_tables,
            self.wasm_module,
            self.ctx_ptr_value,
            &self.cache_builder,
            &self.offsets,
        );
        let TableCache {
            ptr_to_base_ptr,
            ptr_to_bounds,
        } = *cached_tables.entry(table_index).or_insert_with(|| {
            let (ptr_to_base_ptr, ptr_to_bounds) =
                if let Some(local_table_index) = wasm_module.local_table_index(table_index) {
                    let offset = intrinsics.i64_ty.const_int(
                        offsets
                            .vmctx_vmtable_definition_base(local_table_index)
                            .into(),
                        false,
                    );
                    let ptr_to_base_ptr =
                        unsafe { cache_builder.build_gep(ctx_ptr_value, &[offset], "") };
                    let ptr_to_base_ptr = cache_builder
                        .build_bitcast(
                            ptr_to_base_ptr,
                            intrinsics.i8_ptr_ty.ptr_type(AddressSpace::Generic),
                            "",
                        )
                        .into_pointer_value();
                    let offset = intrinsics.i64_ty.const_int(
                        offsets
                            .vmctx_vmtable_definition_current_elements(local_table_index)
                            .into(),
                        false,
                    );
                    let ptr_to_bounds =
                        unsafe { cache_builder.build_gep(ctx_ptr_value, &[offset], "") };
                    let ptr_to_bounds = cache_builder
                        .build_bitcast(ptr_to_bounds, intrinsics.i32_ptr_ty, "")
                        .into_pointer_value();
                    (ptr_to_base_ptr, ptr_to_bounds)
                } else {
                    let offset = intrinsics.i64_ty.const_int(
                        offsets.vmctx_vmtable_import_definition(table_index).into(),
                        false,
                    );
                    let definition_ptr_ptr =
                        unsafe { cache_builder.build_gep(ctx_ptr_value, &[offset], "") };
                    let definition_ptr_ptr = cache_builder
                        .build_bitcast(
                            definition_ptr_ptr,
                            intrinsics.i8_ptr_ty.ptr_type(AddressSpace::Generic),
                            "",
                        )
                        .into_pointer_value();
                    let definition_ptr = cache_builder
                        .build_load(definition_ptr_ptr, "")
                        .into_pointer_value();
                    // TODO: TBAA label

                    let offset = intrinsics
                        .i64_ty
                        .const_int(offsets.vmtable_definition_base().into(), false);
                    let ptr_to_base_ptr =
                        unsafe { cache_builder.build_gep(definition_ptr, &[offset], "") };
                    let ptr_to_base_ptr = cache_builder
                        .build_bitcast(
                            ptr_to_base_ptr,
                            intrinsics.i8_ptr_ty.ptr_type(AddressSpace::Generic),
                            "",
                        )
                        .into_pointer_value();
                    let offset = intrinsics
                        .i64_ty
                        .const_int(offsets.vmtable_definition_current_elements().into(), false);
                    let ptr_to_bounds =
                        unsafe { cache_builder.build_gep(definition_ptr, &[offset], "") };
                    let ptr_to_bounds = cache_builder
                        .build_bitcast(ptr_to_bounds, intrinsics.i32_ptr_ty, "")
                        .into_pointer_value();
                    (ptr_to_base_ptr, ptr_to_bounds)
                };
            TableCache {
                ptr_to_base_ptr,
                ptr_to_bounds,
            }
        });

        (ptr_to_base_ptr, ptr_to_bounds)
    }

    pub fn table(
        &mut self,
        index: TableIndex,
        intrinsics: &Intrinsics<'ctx>,
        module: &Module<'ctx>,
        builder: &Builder<'ctx>,
    ) -> (PointerValue<'ctx>, IntValue<'ctx>) {
        let (ptr_to_base_ptr, ptr_to_bounds) = self.table_prepare(index, intrinsics, module);
        let base_ptr = self
            .cache_builder
            .build_load(ptr_to_base_ptr, "base_ptr")
            .into_pointer_value();
        let bounds = self
            .cache_builder
            .build_load(ptr_to_bounds, "bounds")
            .into_int_value();
        tbaa_label(
            module,
            intrinsics,
            format!("table_base_ptr {}", index.index()),
            base_ptr.as_instruction_value().unwrap(),
        );
        tbaa_label(
            module,
            intrinsics,
            format!("table_bounds {}", index.index()),
            bounds.as_instruction_value().unwrap(),
        );
        (base_ptr, bounds)
    }

    pub fn dynamic_sigindex(
        &mut self,
        index: SignatureIndex,
        intrinsics: &Intrinsics<'ctx>,
    ) -> IntValue<'ctx> {
        let (cached_sigindices, ctx_ptr_value, cache_builder, offsets) = (
            &mut self.cached_sigindices,
            self.ctx_ptr_value,
            &self.cache_builder,
            &self.offsets,
        );
        *cached_sigindices.entry(index).or_insert_with(|| {
            let byte_offset = intrinsics
                .i64_ty
                .const_int(offsets.vmctx_vmshared_signature_id(index).into(), false);
            let sigindex_ptr = unsafe {
                cache_builder.build_gep(ctx_ptr_value, &[byte_offset], "dynamic_sigindex")
            };
            let sigindex_ptr = cache_builder
                .build_bitcast(sigindex_ptr, intrinsics.i32_ptr_ty, "")
                .into_pointer_value();

            cache_builder
                .build_load(sigindex_ptr, "sigindex")
                .into_int_value()
            // TODO: tbaa
        })
    }

    pub fn global(
        &mut self,
        index: GlobalIndex,
        intrinsics: &Intrinsics<'ctx>,
    ) -> GlobalCache<'ctx> {
        let (cached_globals, wasm_module, ctx_ptr_value, cache_builder, offsets) = (
            &mut self.cached_globals,
            self.wasm_module,
            self.ctx_ptr_value,
            &self.cache_builder,
            &self.offsets,
        );
        *cached_globals.entry(index).or_insert_with(|| {
            let global_type = wasm_module.globals[index];
            let global_value_type = global_type.ty;

            let global_mutability = global_type.mutability;
            let global_ptr = if let Some(local_global_index) = wasm_module.local_global_index(index)
            {
                let offset = offsets.vmctx_vmglobal_definition(local_global_index);
                let offset = intrinsics.i32_ty.const_int(offset.into(), false);
                unsafe { cache_builder.build_gep(ctx_ptr_value, &[offset], "") }
            } else {
                let offset = offsets.vmctx_vmglobal_import(index);
                let offset = intrinsics.i32_ty.const_int(offset.into(), false);
                let global_ptr_ptr =
                    unsafe { cache_builder.build_gep(ctx_ptr_value, &[offset], "") };
                let global_ptr_ptr = cache_builder
                    .build_bitcast(
                        global_ptr_ptr,
                        intrinsics.i32_ptr_ty.ptr_type(AddressSpace::Generic),
                        "",
                    )
                    .into_pointer_value();
                cache_builder
                    .build_load(global_ptr_ptr, "")
                    .into_pointer_value()
                // TODO: tbaa
            };
            let global_ptr = cache_builder
                .build_bitcast(
                    global_ptr,
                    type_to_llvm_ptr(&intrinsics, global_value_type),
                    "",
                )
                .into_pointer_value();

            match global_mutability {
                Mutability::Const => GlobalCache::Const {
                    // TODO: tbaa
                    value: cache_builder.build_load(global_ptr, ""),
                },
                Mutability::Var => GlobalCache::Mut {
                    ptr_to_value: global_ptr,
                },
            }
        })
    }
}

// Given an instruction that operates on memory, mark the access as not aliasing
// other memory accesses which have a different label.
pub fn tbaa_label<'ctx>(
    module: &Module<'ctx>,
    intrinsics: &Intrinsics<'ctx>,
    label: String,
    instruction: InstructionValue<'ctx>,
) {
    // To convey to LLVM that two pointers must be pointing to distinct memory,
    // we use LLVM's Type Based Aliasing Analysis, or TBAA, to mark the memory
    // operations as having different types whose pointers may not alias.
    //
    // See the LLVM documentation at
    //   https://llvm.org/docs/LangRef.html#tbaa-metadata
    //
    // LLVM TBAA supports many features, but we use it in a simple way, with
    // only scalar types that are children of the root node. Every TBAA type we
    // declare is NoAlias with the others. See NoAlias, PartialAlias,
    // MayAlias and MustAlias in the LLVM documentation:
    //   https://llvm.org/docs/AliasAnalysis.html#must-may-and-no-alias-responses

    let context = module.get_context();

    // TODO: ContextRef can't return us the lifetime from module through Deref.
    // This could be fixed once generic_associated_types is stable.
    let context = {
        let context2 = &*context;
        unsafe { std::mem::transmute::<&Context, &'ctx Context>(context2) }
    };

    // `!wasmer_tbaa_root = {}`, the TBAA root node for wasmer.
    let tbaa_root = module
        .get_global_metadata("wasmer_tbaa_root")
        .pop()
        .unwrap_or_else(|| {
            module.add_global_metadata("wasmer_tbaa_root", &context.metadata_node(&[]));
            module.get_global_metadata("wasmer_tbaa_root")[0]
        });

    // Construct (or look up) the type descriptor, for example
    //   `!"local 0" = !{!"local 0", !wasmer_tbaa_root}`.
    let type_label = context.metadata_string(label.as_str());
    let type_tbaa = module
        .get_global_metadata(label.as_str())
        .pop()
        .unwrap_or_else(|| {
            module.add_global_metadata(
                label.as_str(),
                &context.metadata_node(&[type_label.into(), tbaa_root.into()]),
            );
            module.get_global_metadata(label.as_str())[0]
        });

    // Construct (or look up) the access tag, which is a struct of the form
    // (base type, access type, offset).
    //
    // "If BaseTy is a scalar type, Offset must be 0 and BaseTy and AccessTy
    // must be the same".
    //   -- https://llvm.org/docs/LangRef.html#tbaa-metadata
    let label = label + "_memop";
    let type_tbaa = module
        .get_global_metadata(label.as_str())
        .pop()
        .unwrap_or_else(|| {
            module.add_global_metadata(
                label.as_str(),
                &context.metadata_node(&[
                    type_tbaa.into(),
                    type_tbaa.into(),
                    intrinsics.i64_zero.into(),
                ]),
            );
            module.get_global_metadata(label.as_str())[0]
        });

    // Attach the access tag to the instruction.
    let tbaa_kind = context.get_kind_id("tbaa");
    instruction.set_metadata(type_tbaa, tbaa_kind);
}

pub fn func_type_to_llvm<'ctx>(
    context: &'ctx Context,
    intrinsics: &Intrinsics<'ctx>,
    fntype: &FuncType,
) -> FunctionType<'ctx> {
    let user_param_types = fntype
        .params()
        .iter()
        .map(|&ty| type_to_llvm(intrinsics, ty));
    let param_types: Vec<_> = std::iter::repeat(intrinsics.ctx_ptr_ty.as_basic_type_enum())
        .take(2)
        .chain(user_param_types)
        .collect();

    match fntype.results() {
        &[] => intrinsics.void_ty.fn_type(&param_types, false),
        &[single_value] => type_to_llvm(intrinsics, single_value).fn_type(&param_types, false),
        returns @ _ => {
            let basic_types: Vec<_> = returns
                .iter()
                .map(|&ty| type_to_llvm(intrinsics, ty))
                .collect();

            context
                .struct_type(&basic_types, false)
                .fn_type(&param_types, false)
        }
    }
}

pub fn type_to_llvm<'ctx>(intrinsics: &Intrinsics<'ctx>, ty: Type) -> BasicTypeEnum<'ctx> {
    match ty {
        Type::I32 => intrinsics.i32_ty.as_basic_type_enum(),
        Type::I64 => intrinsics.i64_ty.as_basic_type_enum(),
        Type::F32 => intrinsics.f32_ty.as_basic_type_enum(),
        Type::F64 => intrinsics.f64_ty.as_basic_type_enum(),
        Type::V128 => intrinsics.i128_ty.as_basic_type_enum(),
        Type::AnyRef => unimplemented!("anyref in the llvm backend"),
        Type::FuncRef => unimplemented!("funcref in the llvm backend"),
    }
}
