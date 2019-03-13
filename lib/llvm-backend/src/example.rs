use inkwell::OptimizationLevel;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::execution_engine::{ExecutionEngine, JitFunction};
use inkwell::module::Module;
use inkwell::targets::{InitializationConfig, Target};
use std::error::Error;

/// Convenience type alias for the `sum` function.
///
/// Calling this is innately `unsafe` because there's no guarantee it doesn't
/// do `unsafe` operations internally.
type SumFunc = unsafe extern "C" fn(u64, u64, u64) -> u64;

#[test]
fn test_sum() -> Result<(), Box<Error>> {
    let context = Context::create();
    let module = context.create_module("sum");
    let builder = context.create_builder();
    let execution_engine = module.create_jit_execution_engine(OptimizationLevel::Aggressive)?;

    let sum = jit_compile_sum(&context, &module, &builder, &execution_engine)
        .ok_or("Unable to JIT compile `sum`")?;

    let x = 1u64;
    let y = 2u64;
    let z = 3u64;

    unsafe {
        println!("{} + {} + {} = {}", x, y, z, sum.call(x, y, z));
        assert_eq!(sum.call(x, y, z), x + y + z);
    }

    Ok(())
}

fn jit_compile_sum(
    context: &Context,
    module: &Module,
    builder: &Builder,
    execution_engine: &ExecutionEngine,
) -> Option<JitFunction<SumFunc>> {
    let i64_type = context.i64_type();
    let fn_type = i64_type.fn_type(&[i64_type.into(), i64_type.into(), i64_type.into()], false);

    let function = module.add_function("sum", fn_type, None);
    let basic_block = context.append_basic_block(&function, "entry");

    builder.position_at_end(&basic_block);

    let x = function.get_nth_param(0)?.into_int_value();
    let y = function.get_nth_param(1)?.into_int_value();
    let z = function.get_nth_param(2)?.into_int_value();

    let sum = builder.build_int_add(x, y, "sum");
    let sum = builder.build_int_add(sum, z, "sum");

    builder.build_return(Some(&sum));

    unsafe { execution_engine.get_function("sum").ok() }
}
