pub mod utils;

use wasmer_clif_backend::CraneliftCompiler;
use wasmer_runtime::{
    self as runtime,
    error::{CallResult, CallError, Result},
    import::Imports,
    instance::Instance,
    module::Module,
    types::{Value, Type},
};
use std::panic;
use wasmer_emscripten::is_emscripten_module;

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
pub fn instantiate(buffer_source: &[u8], import_object: Imports) -> Result<ResultObject> {
    debug!("webassembly - compiling module");
    let module = compile(&buffer_source[..])?;

    debug!("webassembly - instantiating");
    let instance = module.instantiate(import_object)?;

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
    _import_object: Imports,
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
    let compiler = CraneliftCompiler::new();
    let module = runtime::compile(buffer_source, &compiler)?;
    Ok(module)
}

/// Performs common instance operations needed when an instance is first run
/// including data setup, handling arguments and calling a main function
pub fn run_instance(
    module: &Module,
    instance: &mut Instance,
    _path: &str,
    args: Vec<&str>,
) -> CallResult<()> {
    // Get main name.
    let main_name = if is_emscripten_module(module) {
        "_main"
    } else {
        "main"
    };

    // Get main functin arguments.
    let main_args = get_main_args(main_name, args, instance)?;

    // Call main function
    instance.call(main_name, &main_args[..])?;

    // TODO atinit and atexit for emscripten
    Ok(())
}

/// Passes arguments from the host to the WebAssemblky instance.
fn get_main_args(main_name: &str, _args: Vec<&str>, instance: &Instance) -> CallResult<Vec<Value>> {
    // Getting main function signature.
    let func_index = instance.get_func_index(main_name)?;
    let func_sig = instance.get_func_signature(func_index);
    let params = func_sig.params;
    let params_len = params.len();

    // Check for a (i32, i32) sig.
    if params_len == 2 && params[0] == Type::I32 && params[1] == Type::I32 {
        // TODO: Copy args to wasm memory.
        return Ok(vec![Value::I32(0), Value::I32(0)])
    }

    // Check for a () sig.
    if params_len == 0 {
        return Ok(vec![])
    }

    Err(CallError::BadMainSignature { found: params }.into())
}
