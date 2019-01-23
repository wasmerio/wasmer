pub mod utils;

use std::{mem::size_of, panic, slice};
use wasmer_runtime::{
    self as runtime,
    error::CallError,
    error::{CallResult, Result},
    import::ImportObject,
    instance::Instance,
    module::Module,
    types::{FuncSig, Type, Value},
};

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
pub fn instantiate(buffer_source: &[u8], import_object: ImportObject) -> Result<ResultObject> {
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
    _path: &str,
    args: Vec<&str>,
) -> CallResult<()> {
    let main_name = if is_emscripten_module(module) {
        "_main"
    } else {
        "main"
    };

    // Get main arguments.
    let main_args = get_main_args(main_name, args, instance).unwrap();

    // Call main function with the arguments.
    instance.call(main_name, &main_args)?;

    // TODO atinit and atexit for emscripten

    Ok(())
}

/// Passes arguments from the host to the WebAssembly instance.
fn get_main_args(
    main_name: &str,
    args: Vec<&str>,
    instance: &mut Instance,
) -> CallResult<Vec<Value>> {
    // Getting main function signature.
    let func_sig = instance.get_signature(main_name)?;
    let params = &func_sig.params;

    // Check for a () or (i32, i32) sig.
    match params.as_slice() {
        &[Type::I32, Type::I32] => {
            // Copy strings into wasm memory and get addresses to them.
            let string_addresses = args
                .iter()
                .map(|string| copy_string_into_wasm(instance, (*string).to_string()).unwrap())
                .collect();

            // Create a wasm array to the strings.
            let array = create_wasm_array(instance, string_addresses).unwrap();

            Ok(vec![
                Value::I32(array as i32),
                Value::I32(args.len() as i32),
            ])
        }
        &[] => Ok(vec![]),
        _ => Err(CallError::Signature {
            expected: FuncSig {
                params: vec![Type::I32, Type::I32],
                returns: vec![],
            },
            found: params.to_vec(),
        }
        .into()),
    }
}

/// Copy rust string to wasm instance.
fn copy_string_into_wasm(instance: &mut Instance, string: String) -> CallResult<u32> {
    let string_len = string.len();

    let space_offset = instance
        .call("_malloc", &[Value::I32((string_len as i32) + 1)])
        .unwrap();

    let space_offset = match space_offset.as_slice() {
        &[Value::I32(res)] => Some(res as u32),
        _ => None,
    }.unwrap();

    let raw_memory = instance.inner.vmctx.memory(0)[space_offset as usize] as *mut u8;

    let slice = unsafe { slice::from_raw_parts_mut(raw_memory, string_len) };

    for (byte, loc) in string.bytes().zip(slice.iter_mut()) {
        *loc = byte;
    }

    unsafe { *raw_memory.add(string_len) = 0 };

    Ok(space_offset)
}

/// Create a pointer to an array of items in a wasm memory
fn create_wasm_array(instance: &mut Instance, values: Vec<u32>) -> CallResult<u32> {
    let values_len = values.len();

    // Space to store pointers to values
    let values_offset = instance
        .call(
            "_malloc",
            &[Value::I32((size_of::<u32>() * values.len()) as i32)],
        )
        .unwrap();

    let values_offset = match values_offset.as_slice() {
        &[Value::I32(res)] => Some(res as u32),
        _ => None,
    }.unwrap();

    let raw_memory = instance.inner.vmctx.memory(0)[values_offset as usize] as *mut u32;

    let slice = unsafe { slice::from_raw_parts_mut(raw_memory, values_len) };

    for (value, loc) in values.iter().zip(slice.iter_mut()) {
        *loc = value.clone();
    }

    // Space to store pointer to array
    let array_offset = instance
        .call("_malloc", &[Value::I32(size_of::<u32>() as i32)])
        .unwrap();

    let array_offset = match array_offset.as_slice() {
        &[Value::I32(res)] => Some(res as u32),
        _ => None,
    }.unwrap();

    let raw_memory = instance.inner.vmctx.memory(0)[values_offset as usize] as *mut u32;

    unsafe { *raw_memory = values_offset };

    Ok(array_offset)
}
