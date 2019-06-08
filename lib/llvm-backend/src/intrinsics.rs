use hashbrown::HashMap;
use inkwell::{
    builder::Builder,
    context::Context,
    module::Module,
    types::{BasicType, FloatType, FunctionType, IntType, PointerType, StructType, VoidType},
    values::{BasicValue, BasicValueEnum, FloatValue, FunctionValue, IntValue, PointerValue},
    AddressSpace,
};
use std::marker::PhantomData;
use wasmer_runtime_core::{
    memory::MemoryType,
    module::ModuleInfo,
    structures::TypedIndex,
    types::{
        GlobalIndex, ImportedFuncIndex, LocalFuncIndex, LocalOrImport, MemoryIndex, SigIndex,
        TableIndex, Type,
    },
    vm::Ctx,
};

fn type_to_llvm_ptr(intrinsics: &Intrinsics, ty: Type) -> PointerType {
    match ty {
        Type::I32 => intrinsics.i32_ptr_ty,
        Type::I64 => intrinsics.i64_ptr_ty,
        Type::F32 => intrinsics.f32_ptr_ty,
        Type::F64 => intrinsics.f64_ptr_ty,
    }
}

pub struct Intrinsics {
    pub ctlz_i32: FunctionValue,
    pub ctlz_i64: FunctionValue,

    pub cttz_i32: FunctionValue,
    pub cttz_i64: FunctionValue,

    pub ctpop_i32: FunctionValue,
    pub ctpop_i64: FunctionValue,

    pub sqrt_f32: FunctionValue,
    pub sqrt_f64: FunctionValue,

    pub minimum_f32: FunctionValue,
    pub minimum_f64: FunctionValue,

    pub maximum_f32: FunctionValue,
    pub maximum_f64: FunctionValue,

    pub ceil_f32: FunctionValue,
    pub ceil_f64: FunctionValue,

    pub floor_f32: FunctionValue,
    pub floor_f64: FunctionValue,

    pub trunc_f32: FunctionValue,
    pub trunc_f64: FunctionValue,

    pub nearbyint_f32: FunctionValue,
    pub nearbyint_f64: FunctionValue,

    pub fabs_f32: FunctionValue,
    pub fabs_f64: FunctionValue,

    pub copysign_f32: FunctionValue,
    pub copysign_f64: FunctionValue,

    pub expect_i1: FunctionValue,
    pub trap: FunctionValue,

    pub void_ty: VoidType,
    pub i1_ty: IntType,
    pub i8_ty: IntType,
    pub i16_ty: IntType,
    pub i32_ty: IntType,
    pub i64_ty: IntType,
    pub f32_ty: FloatType,
    pub f64_ty: FloatType,

    pub i8_ptr_ty: PointerType,
    pub i16_ptr_ty: PointerType,
    pub i32_ptr_ty: PointerType,
    pub i64_ptr_ty: PointerType,
    pub f32_ptr_ty: PointerType,
    pub f64_ptr_ty: PointerType,

    pub anyfunc_ty: StructType,

    pub i1_zero: IntValue,
    pub i32_zero: IntValue,
    pub i64_zero: IntValue,
    pub f32_zero: FloatValue,
    pub f64_zero: FloatValue,

    pub trap_unreachable: BasicValueEnum,
    pub trap_call_indirect_sig: BasicValueEnum,
    pub trap_call_indirect_oob: BasicValueEnum,
    pub trap_memory_oob: BasicValueEnum,
    pub trap_illegal_arithmetic: BasicValueEnum,

    // VM intrinsics.
    pub memory_grow_dynamic_local: FunctionValue,
    pub memory_grow_static_local: FunctionValue,
    pub memory_grow_shared_local: FunctionValue,
    pub memory_grow_dynamic_import: FunctionValue,
    pub memory_grow_static_import: FunctionValue,
    pub memory_grow_shared_import: FunctionValue,

    pub memory_size_dynamic_local: FunctionValue,
    pub memory_size_static_local: FunctionValue,
    pub memory_size_shared_local: FunctionValue,
    pub memory_size_dynamic_import: FunctionValue,
    pub memory_size_static_import: FunctionValue,
    pub memory_size_shared_import: FunctionValue,

    pub throw_trap: FunctionValue,

    pub ctx_ptr_ty: PointerType,
}

impl Intrinsics {
    pub fn declare(module: &Module, context: &Context) -> Self {
        let void_ty = context.void_type();
        let i1_ty = context.bool_type();
        let i8_ty = context.i8_type();
        let i16_ty = context.i16_type();
        let i32_ty = context.i32_type();
        let i64_ty = context.i64_type();
        let f32_ty = context.f32_type();
        let f64_ty = context.f64_type();

        let i8_ptr_ty = i8_ty.ptr_type(AddressSpace::Generic);
        let i16_ptr_ty = i16_ty.ptr_type(AddressSpace::Generic);
        let i32_ptr_ty = i32_ty.ptr_type(AddressSpace::Generic);
        let i64_ptr_ty = i64_ty.ptr_type(AddressSpace::Generic);
        let f32_ptr_ty = f32_ty.ptr_type(AddressSpace::Generic);
        let f64_ptr_ty = f64_ty.ptr_type(AddressSpace::Generic);

        let i1_zero = i1_ty.const_int(0, false);
        let i32_zero = i32_ty.const_int(0, false);
        let i64_zero = i64_ty.const_int(0, false);
        let f32_zero = f32_ty.const_float(0.0);
        let f64_zero = f64_ty.const_float(0.0);

        let i1_ty_basic = i1_ty.as_basic_type_enum();
        let i32_ty_basic = i32_ty.as_basic_type_enum();
        let i64_ty_basic = i64_ty.as_basic_type_enum();
        let f32_ty_basic = f32_ty.as_basic_type_enum();
        let f64_ty_basic = f64_ty.as_basic_type_enum();
        let i8_ptr_ty_basic = i8_ptr_ty.as_basic_type_enum();

        let ctx_ty = context.opaque_struct_type("ctx");
        let ctx_ptr_ty = ctx_ty.ptr_type(AddressSpace::Generic);

        let local_memory_ty =
            context.struct_type(&[i8_ptr_ty_basic, i64_ty_basic, i8_ptr_ty_basic], false);
        let local_table_ty = local_memory_ty;
        let local_global_ty = i64_ty;
        let imported_func_ty =
            context.struct_type(&[i8_ptr_ty_basic, ctx_ptr_ty.as_basic_type_enum()], false);
        let sigindex_ty = i32_ty;
        let rt_intrinsics_ty = void_ty;
        let stack_lower_bound_ty = i8_ty;
        let memory_base_ty = i8_ty;
        let memory_bound_ty = void_ty;
        let internals_ty = i64_ty;
        let local_function_ty = i8_ptr_ty;

        let anyfunc_ty = context.struct_type(
            &[
                i8_ptr_ty_basic,
                ctx_ptr_ty.as_basic_type_enum(),
                sigindex_ty.as_basic_type_enum(),
            ],
            false,
        );

        ctx_ty.set_body(
            &[
                local_memory_ty
                    .ptr_type(AddressSpace::Generic)
                    .ptr_type(AddressSpace::Generic)
                    .as_basic_type_enum(),
                local_table_ty
                    .ptr_type(AddressSpace::Generic)
                    .ptr_type(AddressSpace::Generic)
                    .as_basic_type_enum(),
                local_global_ty
                    .ptr_type(AddressSpace::Generic)
                    .ptr_type(AddressSpace::Generic)
                    .as_basic_type_enum(),
                local_memory_ty
                    .ptr_type(AddressSpace::Generic)
                    .ptr_type(AddressSpace::Generic)
                    .as_basic_type_enum(),
                local_table_ty
                    .ptr_type(AddressSpace::Generic)
                    .ptr_type(AddressSpace::Generic)
                    .as_basic_type_enum(),
                local_global_ty
                    .ptr_type(AddressSpace::Generic)
                    .ptr_type(AddressSpace::Generic)
                    .as_basic_type_enum(),
                imported_func_ty
                    .ptr_type(AddressSpace::Generic)
                    .as_basic_type_enum(),
                sigindex_ty
                    .ptr_type(AddressSpace::Generic)
                    .as_basic_type_enum(),
                rt_intrinsics_ty
                    .ptr_type(AddressSpace::Generic)
                    .as_basic_type_enum(),
                stack_lower_bound_ty
                    .ptr_type(AddressSpace::Generic)
                    .as_basic_type_enum(),
                memory_base_ty
                    .ptr_type(AddressSpace::Generic)
                    .as_basic_type_enum(),
                memory_bound_ty
                    .ptr_type(AddressSpace::Generic)
                    .as_basic_type_enum(),
                internals_ty
                    .ptr_type(AddressSpace::Generic)
                    .as_basic_type_enum(),
                local_function_ty
                    .ptr_type(AddressSpace::Generic)
                    .as_basic_type_enum(),
            ],
            false,
        );

        let ret_i32_take_i32_i1 = i32_ty.fn_type(&[i32_ty_basic, i1_ty_basic], false);
        let ret_i64_take_i64_i1 = i64_ty.fn_type(&[i64_ty_basic, i1_ty_basic], false);

        let ret_i32_take_i32 = i32_ty.fn_type(&[i32_ty_basic], false);
        let ret_i64_take_i64 = i64_ty.fn_type(&[i64_ty_basic], false);

        let ret_f32_take_f32 = f32_ty.fn_type(&[f32_ty_basic], false);
        let ret_f64_take_f64 = f64_ty.fn_type(&[f64_ty_basic], false);

        let ret_f32_take_f32_f32 = f32_ty.fn_type(&[f32_ty_basic, f32_ty_basic], false);
        let ret_f64_take_f64_f64 = f64_ty.fn_type(&[f64_ty_basic, f64_ty_basic], false);

        let ret_i32_take_ctx_i32_i32 = i32_ty.fn_type(
            &[ctx_ptr_ty.as_basic_type_enum(), i32_ty_basic, i32_ty_basic],
            false,
        );
        let ret_i32_take_ctx_i32 =
            i32_ty.fn_type(&[ctx_ptr_ty.as_basic_type_enum(), i32_ty_basic], false);

        let ret_i1_take_i1_i1 = i1_ty.fn_type(&[i1_ty_basic, i1_ty_basic], false);

        Self {
            ctlz_i32: module.add_function("llvm.ctlz.i32", ret_i32_take_i32_i1, None),
            ctlz_i64: module.add_function("llvm.ctlz.i64", ret_i64_take_i64_i1, None),

            cttz_i32: module.add_function("llvm.cttz.i32", ret_i32_take_i32_i1, None),
            cttz_i64: module.add_function("llvm.cttz.i64", ret_i64_take_i64_i1, None),

            ctpop_i32: module.add_function("llvm.ctpop.i32", ret_i32_take_i32, None),
            ctpop_i64: module.add_function("llvm.ctpop.i64", ret_i64_take_i64, None),

            sqrt_f32: module.add_function("llvm.sqrt.f32", ret_f32_take_f32, None),
            sqrt_f64: module.add_function("llvm.sqrt.f64", ret_f64_take_f64, None),

            minimum_f32: module.add_function("llvm.minnum.f32", ret_f32_take_f32_f32, None),
            minimum_f64: module.add_function("llvm.minnum.f64", ret_f64_take_f64_f64, None),

            maximum_f32: module.add_function("llvm.maxnum.f32", ret_f32_take_f32_f32, None),
            maximum_f64: module.add_function("llvm.maxnum.f64", ret_f64_take_f64_f64, None),

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

            copysign_f32: module.add_function("llvm.copysign.f32", ret_f32_take_f32_f32, None),
            copysign_f64: module.add_function("llvm.copysign.f64", ret_f64_take_f64_f64, None),

            expect_i1: module.add_function("llvm.expect.i1", ret_i1_take_i1_i1, None),
            trap: module.add_function("llvm.trap", void_ty.fn_type(&[], false), None),

            void_ty,
            i1_ty,
            i8_ty,
            i16_ty,
            i32_ty,
            i64_ty,
            f32_ty,
            f64_ty,

            i8_ptr_ty,
            i16_ptr_ty,
            i32_ptr_ty,
            i64_ptr_ty,
            f32_ptr_ty,
            f64_ptr_ty,

            anyfunc_ty,

            i1_zero,
            i32_zero,
            i64_zero,
            f32_zero,
            f64_zero,

            trap_unreachable: i32_zero.as_basic_value_enum(),
            trap_call_indirect_sig: i32_ty.const_int(1, false).as_basic_value_enum(),
            trap_call_indirect_oob: i32_ty.const_int(3, false).as_basic_value_enum(),
            trap_memory_oob: i32_ty.const_int(2, false).as_basic_value_enum(),
            trap_illegal_arithmetic: i32_ty.const_int(4, false).as_basic_value_enum(),

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
            ctx_ptr_ty,
        }
    }
}

#[derive(Clone, Copy)]
pub enum MemoryCache {
    /// The memory moves around.
    Dynamic {
        ptr_to_base_ptr: PointerValue,
        ptr_to_bounds: PointerValue,
    },
    /// The memory is always in the same place.
    Static {
        base_ptr: PointerValue,
        bounds: IntValue,
    },
}

struct TableCache {
    ptr_to_base_ptr: PointerValue,
    ptr_to_bounds: PointerValue,
}

#[derive(Clone, Copy)]
pub enum GlobalCache {
    Mut { ptr_to_value: PointerValue },
    Const { value: BasicValueEnum },
}

struct ImportedFuncCache {
    func_ptr: PointerValue,
    ctx_ptr: PointerValue,
}

pub struct CtxType<'a> {
    ctx_ptr_value: PointerValue,

    info: &'a ModuleInfo,
    cache_builder: Builder,

    cached_memories: HashMap<MemoryIndex, MemoryCache>,
    cached_tables: HashMap<TableIndex, TableCache>,
    cached_sigindices: HashMap<SigIndex, IntValue>,
    cached_globals: HashMap<GlobalIndex, GlobalCache>,
    cached_imported_functions: HashMap<ImportedFuncIndex, ImportedFuncCache>,

    _phantom: PhantomData<&'a FunctionValue>,
}

fn offset_to_index(offset: u8) -> u32 {
    (offset as usize / ::std::mem::size_of::<usize>()) as u32
}

impl<'a> CtxType<'a> {
    pub fn new(
        info: &'a ModuleInfo,
        func_value: &'a FunctionValue,
        cache_builder: Builder,
    ) -> CtxType<'a> {
        CtxType {
            ctx_ptr_value: func_value.get_nth_param(0).unwrap().into_pointer_value(),

            info,
            cache_builder,

            cached_memories: HashMap::new(),
            cached_tables: HashMap::new(),
            cached_sigindices: HashMap::new(),
            cached_globals: HashMap::new(),
            cached_imported_functions: HashMap::new(),

            _phantom: PhantomData,
        }
    }

    pub fn basic(&self) -> BasicValueEnum {
        self.ctx_ptr_value.as_basic_value_enum()
    }

    pub fn memory(&mut self, index: MemoryIndex, intrinsics: &Intrinsics) -> MemoryCache {
        let (cached_memories, info, ctx_ptr_value, cache_builder) = (
            &mut self.cached_memories,
            self.info,
            self.ctx_ptr_value,
            &self.cache_builder,
        );

        *cached_memories.entry(index).or_insert_with(|| {
            let (memory_array_ptr_ptr, index, memory_type) = match index.local_or_import(info) {
                LocalOrImport::Local(local_mem_index) => (
                    unsafe {
                        cache_builder.build_struct_gep(
                            ctx_ptr_value,
                            offset_to_index(Ctx::offset_memories()),
                            "memory_array_ptr_ptr",
                        )
                    },
                    local_mem_index.index() as u64,
                    info.memories[local_mem_index].memory_type(),
                ),
                LocalOrImport::Import(import_mem_index) => (
                    unsafe {
                        cache_builder.build_struct_gep(
                            ctx_ptr_value,
                            offset_to_index(Ctx::offset_imported_memories()),
                            "memory_array_ptr_ptr",
                        )
                    },
                    import_mem_index.index() as u64,
                    info.imported_memories[import_mem_index].1.memory_type(),
                ),
            };

            let memory_array_ptr = cache_builder
                .build_load(memory_array_ptr_ptr, "memory_array_ptr")
                .into_pointer_value();
            let const_index = intrinsics.i32_ty.const_int(index, false);
            let memory_ptr_ptr = unsafe {
                cache_builder.build_in_bounds_gep(
                    memory_array_ptr,
                    &[const_index],
                    "memory_ptr_ptr",
                )
            };
            let memory_ptr = cache_builder
                .build_load(memory_ptr_ptr, "memory_ptr")
                .into_pointer_value();

            let (ptr_to_base_ptr, ptr_to_bounds) = unsafe {
                (
                    cache_builder.build_struct_gep(memory_ptr, 0, "base_ptr"),
                    cache_builder.build_struct_gep(memory_ptr, 1, "bounds_ptr"),
                )
            };

            match memory_type {
                MemoryType::Dynamic => MemoryCache::Dynamic {
                    ptr_to_base_ptr,
                    ptr_to_bounds,
                },
                MemoryType::Static | MemoryType::SharedStatic => MemoryCache::Static {
                    base_ptr: cache_builder
                        .build_load(ptr_to_base_ptr, "base")
                        .into_pointer_value(),
                    bounds: cache_builder
                        .build_load(ptr_to_bounds, "bounds")
                        .into_int_value(),
                },
            }
        })
    }

    pub fn table(
        &mut self,
        index: TableIndex,
        intrinsics: &Intrinsics,
        builder: &Builder,
    ) -> (PointerValue, IntValue) {
        let (cached_tables, info, ctx_ptr_value, cache_builder) = (
            &mut self.cached_tables,
            self.info,
            self.ctx_ptr_value,
            &self.cache_builder,
        );

        let TableCache {
            ptr_to_base_ptr,
            ptr_to_bounds,
        } = *cached_tables.entry(index).or_insert_with(|| {
            let (table_array_ptr_ptr, index) = match index.local_or_import(info) {
                LocalOrImport::Local(local_table_index) => (
                    unsafe {
                        cache_builder.build_struct_gep(
                            ctx_ptr_value,
                            offset_to_index(Ctx::offset_tables()),
                            "table_array_ptr_ptr",
                        )
                    },
                    local_table_index.index() as u64,
                ),
                LocalOrImport::Import(import_table_index) => (
                    unsafe {
                        cache_builder.build_struct_gep(
                            ctx_ptr_value,
                            offset_to_index(Ctx::offset_imported_tables()),
                            "table_array_ptr_ptr",
                        )
                    },
                    import_table_index.index() as u64,
                ),
            };

            let table_array_ptr = cache_builder
                .build_load(table_array_ptr_ptr, "table_array_ptr")
                .into_pointer_value();
            let const_index = intrinsics.i32_ty.const_int(index, false);
            let table_ptr_ptr = unsafe {
                cache_builder.build_in_bounds_gep(table_array_ptr, &[const_index], "table_ptr_ptr")
            };
            let table_ptr = cache_builder
                .build_load(table_ptr_ptr, "table_ptr")
                .into_pointer_value();

            let (ptr_to_base_ptr, ptr_to_bounds) = unsafe {
                (
                    cache_builder.build_struct_gep(table_ptr, 0, "base_ptr"),
                    cache_builder.build_struct_gep(table_ptr, 1, "bounds_ptr"),
                )
            };

            TableCache {
                ptr_to_base_ptr,
                ptr_to_bounds,
            }
        });

        (
            builder
                .build_load(ptr_to_base_ptr, "base_ptr")
                .into_pointer_value(),
            builder.build_load(ptr_to_bounds, "bounds").into_int_value(),
        )
    }

    pub fn local_func(
        &mut self,
        index: LocalFuncIndex,
        fn_ty: FunctionType,
        intrinsics: &Intrinsics,
        builder: &Builder,
    ) -> PointerValue {
        let local_func_array_ptr_ptr = unsafe {
            builder.build_struct_gep(
                self.ctx_ptr_value,
                offset_to_index(Ctx::offset_local_functions()),
                "local_func_array_ptr_ptr",
            )
        };
        let local_func_array_ptr = builder
            .build_load(local_func_array_ptr_ptr, "local_func_array_ptr")
            .into_pointer_value();
        let local_func_ptr_ptr = unsafe {
            builder.build_in_bounds_gep(
                local_func_array_ptr,
                &[intrinsics.i32_ty.const_int(index.index() as u64, false)],
                "local_func_ptr_ptr",
            )
        };
        let local_func_ptr = builder
            .build_load(local_func_ptr_ptr, "local_func_ptr")
            .into_pointer_value();
        builder.build_pointer_cast(
            local_func_ptr,
            fn_ty.ptr_type(AddressSpace::Generic),
            "local_func_ptr",
        )
    }

    pub fn dynamic_sigindex(&mut self, index: SigIndex, intrinsics: &Intrinsics) -> IntValue {
        let (cached_sigindices, ctx_ptr_value, cache_builder) = (
            &mut self.cached_sigindices,
            self.ctx_ptr_value,
            &self.cache_builder,
        );

        *cached_sigindices.entry(index).or_insert_with(|| {
            let sigindex_array_ptr_ptr = unsafe {
                cache_builder.build_struct_gep(
                    ctx_ptr_value,
                    offset_to_index(Ctx::offset_signatures()),
                    "sigindex_array_ptr_ptr",
                )
            };
            let sigindex_array_ptr = cache_builder
                .build_load(sigindex_array_ptr_ptr, "sigindex_array_ptr")
                .into_pointer_value();
            let const_index = intrinsics.i32_ty.const_int(index.index() as u64, false);

            let sigindex_ptr = unsafe {
                cache_builder.build_in_bounds_gep(
                    sigindex_array_ptr,
                    &[const_index],
                    "sigindex_ptr",
                )
            };

            cache_builder
                .build_load(sigindex_ptr, "sigindex")
                .into_int_value()
        })
    }

    pub fn global_cache(&mut self, index: GlobalIndex, intrinsics: &Intrinsics) -> GlobalCache {
        let (cached_globals, ctx_ptr_value, info, cache_builder) = (
            &mut self.cached_globals,
            self.ctx_ptr_value,
            self.info,
            &self.cache_builder,
        );

        *cached_globals.entry(index).or_insert_with(|| {
            let (globals_array_ptr_ptr, index, mutable, wasmer_ty) =
                match index.local_or_import(info) {
                    LocalOrImport::Local(local_global_index) => {
                        let desc = info.globals[local_global_index].desc;
                        (
                            unsafe {
                                cache_builder.build_struct_gep(
                                    ctx_ptr_value,
                                    offset_to_index(Ctx::offset_globals()),
                                    "globals_array_ptr_ptr",
                                )
                            },
                            local_global_index.index() as u64,
                            desc.mutable,
                            desc.ty,
                        )
                    }
                    LocalOrImport::Import(import_global_index) => {
                        let desc = info.imported_globals[import_global_index].1;
                        (
                            unsafe {
                                cache_builder.build_struct_gep(
                                    ctx_ptr_value,
                                    offset_to_index(Ctx::offset_imported_globals()),
                                    "globals_array_ptr_ptr",
                                )
                            },
                            import_global_index.index() as u64,
                            desc.mutable,
                            desc.ty,
                        )
                    }
                };

            let llvm_ptr_ty = type_to_llvm_ptr(intrinsics, wasmer_ty);

            let global_array_ptr = cache_builder
                .build_load(globals_array_ptr_ptr, "global_array_ptr")
                .into_pointer_value();
            let const_index = intrinsics.i32_ty.const_int(index, false);
            let global_ptr_ptr = unsafe {
                cache_builder.build_in_bounds_gep(
                    global_array_ptr,
                    &[const_index],
                    "global_ptr_ptr",
                )
            };
            let global_ptr = cache_builder
                .build_load(global_ptr_ptr, "global_ptr")
                .into_pointer_value();

            let global_ptr_typed =
                cache_builder.build_pointer_cast(global_ptr, llvm_ptr_ty, "global_ptr_typed");

            if mutable {
                GlobalCache::Mut {
                    ptr_to_value: global_ptr_typed,
                }
            } else {
                GlobalCache::Const {
                    value: cache_builder.build_load(global_ptr_typed, "global_value"),
                }
            }
        })
    }

    pub fn imported_func(
        &mut self,
        index: ImportedFuncIndex,
        intrinsics: &Intrinsics,
    ) -> (PointerValue, PointerValue) {
        let (cached_imported_functions, ctx_ptr_value, cache_builder) = (
            &mut self.cached_imported_functions,
            self.ctx_ptr_value,
            &self.cache_builder,
        );

        let imported_func_cache = cached_imported_functions.entry(index).or_insert_with(|| {
            let func_array_ptr_ptr = unsafe {
                cache_builder.build_struct_gep(
                    ctx_ptr_value,
                    offset_to_index(Ctx::offset_imported_funcs()),
                    "imported_func_array_ptr_ptr",
                )
            };
            let func_array_ptr = cache_builder
                .build_load(func_array_ptr_ptr, "func_array_ptr")
                .into_pointer_value();
            let const_index = intrinsics.i32_ty.const_int(index.index() as u64, false);
            let imported_func_ptr = unsafe {
                cache_builder.build_in_bounds_gep(
                    func_array_ptr,
                    &[const_index],
                    "imported_func_ptr",
                )
            };
            let (func_ptr_ptr, ctx_ptr_ptr) = unsafe {
                (
                    cache_builder.build_struct_gep(imported_func_ptr, 0, "func_ptr_ptr"),
                    cache_builder.build_struct_gep(imported_func_ptr, 1, "ctx_ptr_ptr"),
                )
            };

            let func_ptr = cache_builder
                .build_load(func_ptr_ptr, "func_ptr")
                .into_pointer_value();
            let ctx_ptr = cache_builder
                .build_load(ctx_ptr_ptr, "ctx_ptr")
                .into_pointer_value();

            ImportedFuncCache { func_ptr, ctx_ptr }
        });

        (imported_func_cache.func_ptr, imported_func_cache.ctx_ptr)
    }
}
