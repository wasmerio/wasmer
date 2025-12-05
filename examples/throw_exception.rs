use std::sync::{Arc, atomic::AtomicUsize};

use wasmer::{
    DynamicFunctionResult, Function, FunctionEnv, FunctionEnvMut, FunctionType, Instance, Module,
    RuntimeError, Store, Type, TypedFunction, Value, imports, sys::Target, wat2wasm,
};
use wasmer_types::Features;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Let's declare the Wasm module with the text representation.
    let wasm_bytes = wat2wasm(
        br#"
(module
  (tag $exn (import "env" "tag") (param i32))
  (func $log (import "env" "log") (param i32))
  (func $throw1 (import "env" "throw1"))
  (func $throw2 (import "env" "throw2"))

  (func $f (export "f")
    (local $e exnref)
    (block $outer (result i32 exnref)
      (try_table (catch_ref $exn $outer)
        call $throw2
        unreachable
      )
      unreachable
    )
    drop
    call $log
    (block $outer2 (result i32 exnref)
      (try_table (catch_ref $exn $outer2)
        call $throw1
        unreachable
      )
      unreachable
    )
    local.set $e
    call $log
    local.get $e
    throw_ref
  )
)
"#,
    )?;

    // We need an LLVM backend with the exception handling feature enabled.
    let target = Target::default();
    let features = Features::detect_from_wasm(&wasm_bytes).unwrap();

    let config = wasmer_compiler_llvm::LLVM::new();
    let engine = wasmer_compiler::EngineBuilder::new(config)
        .set_features(Some(features))
        .set_target(Some(target))
        .engine();

    // Create a Store.
    let mut store = Store::new(engine);

    // Declare a tag.
    let tag = wasmer::Tag::new(&mut store, vec![Type::I32]);

    // Store the tag in an environment so we can access it in the `throw` function.
    struct MyEnv {
        tag: wasmer::Tag,
    }
    let env = FunctionEnv::new(&mut store, MyEnv { tag: tag.clone() });

    println!("Compiling module...");
    // Let's compile the Wasm module.
    let module = Module::new(&store, wasm_bytes)?;

    let log_count = Arc::new(AtomicUsize::new(0));

    // Create the functions
    let log_wasm_value = {
        let log_count = log_count.clone();
        move |param: i32| {
            println!("Logging from wasm: {param}");
            log_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    };
    let log = Function::new_typed(&mut store, log_wasm_value);

    // Both Function::new_with_env and Function::new_typed_with_env can throw
    // exceptions if they return a Result with a RuntimeError.
    fn throw1(mut env: FunctionEnvMut<MyEnv>, _params: &[Value]) -> DynamicFunctionResult {
        println!("Throwing exception 1");

        // To "throw" an exception from native code, we create a new one and
        // return it wrapped in a `RuntimeError`. The Wasmer runtime will
        // recognize it and will start the unwinding process.
        let (env, mut ctx) = env.data_and_store_mut();
        let exn = wasmer::Exception::new(&mut ctx, &env.tag, &[Value::I32(69)]);
        Err(RuntimeError::exception(&ctx, exn))
    }
    let throw1 =
        Function::new_with_env(&mut store, &env, FunctionType::new(vec![], vec![]), throw1);

    fn throw2(mut env: FunctionEnvMut<MyEnv>) -> Result<(), RuntimeError> {
        println!("Throwing exception 2");
        let (env, mut ctx) = env.data_and_store_mut();
        let exn = wasmer::Exception::new(&mut ctx, &env.tag, &[Value::I32(42)]);
        Err(RuntimeError::exception(&ctx, exn))
    }
    let throw2 = Function::new_typed_with_env(&mut store, &env, throw2);

    // Create an import object.
    let import_object = imports! {
        "env" => {
            "tag" => tag,
            "log" => log,
            "throw1" => throw1,
            "throw2" => throw2,
        }
    };

    println!("Instantiating module...");
    // Let's instantiate the Wasm module.
    let instance = Instance::new(&mut store, &module, &import_object)?;

    // Here we go.
    let f: TypedFunction<(), ()> = instance.exports.get_function("f")?.typed(&store)?;

    println!("Calling `f` function...");
    let result = f.call(&mut store);

    // Now we can inspect the exception, since it was propagated back to us.
    let err = result.unwrap_err();
    let exn = err.to_exception().expect("should be an exception");
    let values = exn.payload(&mut store);
    println!("Caught exception with payload: {values:?}");

    assert_eq!(values, vec![Value::I32(69)]);
    assert_eq!(log_count.load(std::sync::atomic::Ordering::SeqCst), 2);

    Ok(())
}

#[test]
#[cfg(not(windows))]
fn test_throw_exception() -> Result<(), Box<dyn std::error::Error>> {
    main()
}
