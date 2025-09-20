use wasmer::{
    imports, sys::Target, wat2wasm, Function, FunctionEnv, FunctionEnvMut, Instance, Module,
    RuntimeError, Store, Type, TypedFunction, Value,
};
use wasmer_types::Features;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Let's declare the Wasm module with the text representation.
    let wasm_bytes = wat2wasm(
        br#"
(module
  (tag $exn (import "env" "tag") (param i32))
  (func $log (import "env" "log") (param i32))
  (func $throw (import "env" "throw"))

  (func $f (export "f")
    (local $e exnref)
    (block $outer (result i32 exnref)
      (try_table (catch_ref $exn $outer)
        call $throw
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

    // Create the functions
    fn log(param: i32) {
        println!("Logging from wasm: {}", param);
    }
    let log = Function::new_typed(&mut store, log);

    fn throw(mut env: FunctionEnvMut<MyEnv>) -> Result<(), RuntimeError> {
        println!("Throwing exception");

        // To "throw" an exception from native code, we create a new one and
        // return it wrapped in a `RuntimeError`. The Wasmer runtime will
        // recognize it and will start the unwinding process.
        let (env, mut ctx) = env.data_and_store_mut();
        let exn = wasmer::Exception::new(&mut ctx, &env.tag, &[Value::I32(42)]);
        Err(RuntimeError::exception(&ctx, exn))
    }
    let throw = Function::new_typed_with_env(&mut store, &env, throw);

    // Create an import object.
    let import_object = imports! {
        "env" => {
            "tag" => tag,
            "log" => log,
            "throw" => throw,
        }
    };

    println!("Instantiating module...");
    // Let's instantiate the Wasm module.
    let instance = Instance::new(&mut store, &module, &import_object)?;

    // Here we go.
    let f: TypedFunction<(), ()> = instance.exports.get_function("f")?.typed(&mut store)?;

    println!("Calling `f` function...");
    let result = f.call(&mut store);

    // Now we can inspect the exception, since it was propagated back to us.
    let err = result.unwrap_err();
    let exn = err.to_exception().expect("should be an exception");
    let values = exn.payload(&mut store);
    println!("Caught exception with payload: {:?}", values);

    Ok(())
}

#[test]
fn test_exported_function() -> Result<(), Box<dyn std::error::Error>> {
    main()
}
