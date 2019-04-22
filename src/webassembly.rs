use std::panic;
pub use wasmer_runtime::compile_with_config_with;
use wasmer_runtime::{
    self as runtime,
    error::{CallResult, Result},
    ImportObject, Instance, Module,
};
use wasmer_runtime_core::types::Value;

use wasmer_emscripten::run_emscripten_instance;

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

// /// The same as `compile` but takes a `CompilerConfig` for the purpose of
// /// changing the compiler's behavior
// pub fn compile_with_config_with(
//     buffer_source: &[u8],
//     compiler_config: CompilerConfig,
// ) -> Result<Module> {
//     let module = runtime::compile_with_config(buffer_source, compiler_config)?;
//     Ok(module)
// }

/// Performs common instance operations needed when an instance is first run
/// including data setup, handling arguments and calling a main function
pub fn run_instance(
    module: &Module,
    instance: &mut Instance,
    abi: InstanceABI,
    path: &str,
    args: Vec<&str>,
) -> CallResult<()> {
    match abi {
        InstanceABI::Emscripten => {
            run_emscripten_instance(module, instance, path, args)?;
        }
        InstanceABI::WASI => {
            instance.call("_start", &[])?;
        }
        InstanceABI::None => {
            let args: Vec<Value> = args
                .into_iter()
                .map(|x| Value::I32(x.parse().unwrap()))
                .collect();
            instance.call("main", &args)?;
        }
    }
    Ok(())
}
