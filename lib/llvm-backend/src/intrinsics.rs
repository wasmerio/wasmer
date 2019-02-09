use inkwell::{context::Context, module::Module, types::BasicType, values::FunctionValue};

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
}

impl Intrinsics {
    pub fn declare(module: &Module, context: &Context) -> Self {
        let i1_ty = context.bool_type().as_basic_type_enum();
        let i32_ty = context.i32_type().as_basic_type_enum();
        let i64_ty = context.i64_type().as_basic_type_enum();
        let f32_ty = context.f32_type().as_basic_type_enum();
        let f64_ty = context.f64_type().as_basic_type_enum();

        let ret_i32_take_i32_i1 = i32_ty.fn_type(&[i32_ty, i1_ty], false);
        let ret_i64_take_i64_i1 = i64_ty.fn_type(&[i64_ty, i1_ty], false);

        let ret_i32_take_i32 = i32_ty.fn_type(&[i32_ty], false);
        let ret_i64_take_i64 = i64_ty.fn_type(&[i64_ty], false);

        let ret_f32_take_f32 = f32_ty.fn_type(&[f32_ty], false);
        let ret_f64_take_f64 = f64_ty.fn_type(&[f64_ty], false);

        let ret_f32_take_f32_f32 = f32_ty.fn_type(&[f32_ty, f32_ty], false);
        let ret_f64_take_f64_f64 = f64_ty.fn_type(&[f64_ty, f64_ty], false);

        Self {
            ctlz_i32: module.add_function("llvm.ctlz.i32", ret_i32_take_i32_i1, None),
            ctlz_i64: module.add_function("llvm.ctlz.i64", ret_i64_take_i64_i1, None),

            cttz_i32: module.add_function("llvm.cttz.i32", ret_i32_take_i32_i1, None),
            cttz_i64: module.add_function("llvm.cttz.i64", ret_i64_take_i64_i1, None),

            ctpop_i32: module.add_function("llvm.ctpop.i32", ret_i32_take_i32, None),
            ctpop_i64: module.add_function("llvm.ctpop.i64", ret_i64_take_i64, None),

            sqrt_f32: module.add_function("llvm.sqrt.f32", ret_f32_take_f32, None),
            sqrt_f64: module.add_function("llvm.sqrt.f64", ret_f64_take_f64, None),

            minimum_f32: module.add_function("llvm.minimum.f32", ret_f32_take_f32_f32, None),
            minimum_f64: module.add_function("llvm.minimum.f64", ret_f64_take_f64_f64, None),

            maximum_f32: module.add_function("llvm.maximum.f32", ret_f32_take_f32_f32, None),
            maximum_f64: module.add_function("llvm.maximum.f64", ret_f64_take_f64_f64, None),

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
        }
    }
}
