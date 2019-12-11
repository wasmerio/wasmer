use std::panic;
use wasmer_runtime::{
    self as runtime,
    error::{CallError, CallResult, ResolveError, Result},
    ImportObject, Instance, Module,
};
use wasmer_runtime_core::{
    export::Export,
    types::{Type, Value},
};

use wasmer_emscripten::{is_emscripten_module, run_emscripten_instance};
pub use wasmer_runtime::compile_with_config_with;


pub struct ResultObject {
    /// A webassembly::Module object representing the compiled WebAssembly module.
    /// This Module can be instantiated again
    pub module: Module,
    /// A webassembly::Instance object that contains all the Exported WebAssembly
    /// functions.
    pub instance: Box<Instance>,
}

#[derive(PartialEq)]
pub enum InstanceABI {
    Emscripten,
    WASI,
    None,
}

/// The webassembly::instantiate() function allows you to compile and
/// instantiate WebAssembly code
/// Params:
/// * `buffer_source`: A `Vec<u8>` containing the
///   binary code of the .wasm module you want to compile.
/// * `import_object`: An object containing the values to be imported
///   into the newly-created Instance, such as functions or
///   webassembly::Memory objects. There must be one matching property
///   for each declared import of the compiled module or else a
///   webassembly::LinkError is thrown.
/// Errors:
/// If the operation fails, the Result rejects with a
/// webassembly::CompileError, webassembly::LinkError, or
/// webassembly::RuntimeError, depending on the cause of the failure.
pub fn instantiate(buffer_source: &[u8], import_object: ImportObject) -> Result<ResultObject> {
    debug!("webassembly - compiling module");
    let module = compile(&buffer_source[..])?;

    debug!("webassembly - instantiating");
    let instance = module.instantiate(&import_object)?;

    debug!("webassembly - instance created");
    Ok(ResultObject {
        module,
        instance: Box::new(instance),
    })
}

/// The webassembly::instantiate_streaming() function compiles and instantiates
/// a WebAssembly module directly from a streamed underlying source.
/// This is the most efficient, optimized way to load wasm code.
pub fn instantiate_streaming(
    _buffer_source: Vec<u8>,
    _import_object: ImportObject,
) -> Result<ResultObject> {
    unimplemented!();
}

/// The webassembly::compile() function compiles a webassembly::Module
/// from WebAssembly binary code.  This function is useful if it
/// is necessary to a compile a module before it can be instantiated
/// (otherwise, the webassembly::instantiate() function should be used).
/// Params:
/// * `buffer_source`: A `Vec<u8>` containing the
///   binary code of the .wasm module you want to compile.
/// Errors:
/// If the operation fails, the Result rejects with a
/// webassembly::CompileError.
pub fn compile(buffer_source: &[u8]) -> Result<Module> {
    let module = runtime::compile(buffer_source)?;
    Ok(module)
}

/// Performs common instance operations needed when an instance is first run
/// including data setup, handling arguments and calling a main function
pub fn run_instance(
    module: &Module,
    instance: &mut Instance,
    path: &str,
    args: Vec<&str>,
) -> CallResult<()> {
    if is_emscripten_module(module) {
        run_emscripten_instance(module, instance, path, args)?;

        Ok(())
    } else {
        instance
            .exports()
            .find_map(
                |(export_name, export)| match (export_name.as_ref(), export) {
                    ("main", Export::Function { signature, .. }) => Some(signature),
                    _ => None,
                },
            )
            .ok_or_else(|| {
                CallError::Resolve(ResolveError::ExportNotFound {
                    name: "main".to_string(),
                })
            })
            .and_then(|signature| {
                let signature = signature.clone();
                let parameter_types = signature.params();

                if args.len() != parameter_types.len() {
                    Err(CallError::Resolve(ResolveError::Signature {
                        expected: (*signature).clone(),
                        found: args.iter().map(|_| Type::I32).collect(),
                    }))
                } else {
                    args.iter()
                        .enumerate()
                        .try_fold(
                            Vec::with_capacity(args.len()),
                            |mut accumulator, (nth, argument)| {
                                if let Some(value) = match parameter_types[nth] {
                                    Type::I32 => argument
                                        .parse::<i32>()
                                        .map(|v| Some(Value::I32(v)))
                                        .unwrap_or_else(|_| {
                                            eprintln!(
                                                "Failed to parse `{:?}` as an `i32`",
                                                argument
                                            );
                                            None
                                        }),
                                    Type::I64 => argument
                                        .parse::<i64>()
                                        .map(|v| Some(Value::I64(v)))
                                        .unwrap_or_else(|_| {
                                            eprintln!(
                                                "Failed to parse `{:?}` as an `i64`",
                                                argument
                                            );
                                            None
                                        }),
                                    Type::F32 => argument
                                        .parse::<f32>()
                                        .map(|v| Some(Value::F32(v)))
                                        .unwrap_or_else(|_| {
                                            eprintln!(
                                                "Failed to parse `{:?}` as an `f32`",
                                                argument
                                            );
                                            None
                                        }),
                                    Type::F64 => argument
                                        .parse::<f64>()
                                        .map(|v| Some(Value::F64(v)))
                                        .unwrap_or_else(|_| {
                                            eprintln!(
                                                "Failed to parse `{:?}` as an `f64`",
                                                argument
                                            );
                                            None
                                        }),
                                } {
                                    accumulator.push(value);

                                    Some(accumulator)
                                } else {
                                    None
                                }
                            },
                        )
                        .map_or_else(
                            || {
                                Err(CallError::Resolve(ResolveError::ExportWrongType {
                                    name: "main".to_string(),
                                }))
                            },
                            |arguments| Ok(arguments),
                        )
                }
            })
            .map(|arguments| match instance.call("main", &arguments[..]) {
                Ok(result) => result
                    .iter()
                    .enumerate()
                    .for_each(|(nth, value)| match value {
                        Value::I32(e) => println!("result_{} = i32 : {}", nth, e),
                        Value::I64(e) => println!("result_{} = i64 : {}", nth, e),
                        Value::F32(e) => println!("result_{} = f32 : {}", nth, e),
                        Value::F64(e) => println!("result_{} = f64 : {}", nth, e),
                    }),
                Err(error) => eprintln!("{}", error),
            })
    }
}
