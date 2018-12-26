pub mod errors;
pub mod import_object;
pub mod instance;
pub mod libcalls;
pub mod memory;
pub mod module;
pub mod relocation;
pub mod utils;
pub mod vmcontext;
pub mod vmoffsets;

use cranelift_codegen::{
    isa,
    settings::{self, Configurable},
};
use cranelift_wasm::ModuleEnvironment;
use std::io::{self, Write};
use std::panic;
use std::str::FromStr;
use target_lexicon;
use wasmparser;
use wasmparser::WasmDecoder;

pub use self::errors::{Error, ErrorKind};
pub use self::import_object::{ImportObject, ImportValue};
pub use self::instance::{Instance, InstanceABI, InstanceOptions};
pub use self::memory::LinearMemory;
pub use self::module::{Export, Module, ModuleInfo};

use crate::apis::emscripten::{allocate_cstr_on_stack, allocate_on_stack, is_emscripten_module};

pub struct ResultObject {
    /// A webassembly::Module object representing the compiled WebAssembly module.
    /// This Module can be instantiated again
    pub module: Module,
    /// A webassembly::Instance object that contains all the Exported WebAssembly
    /// functions.
    pub instance: Instance,
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
pub fn instantiate(
    buffer_source: Vec<u8>,
    import_object: ImportObject<&str, &str>,
    options: Option<InstanceOptions>,
) -> Result<ResultObject, ErrorKind> {
    let isa = get_isa();
    let module = compile(buffer_source)?;

    let abi = if is_emscripten_module(&module) {
        InstanceABI::Emscripten
    } else {
        InstanceABI::None
    };

    let options = options.unwrap_or_else(|| InstanceOptions {
        mock_missing_imports: false,
        mock_missing_globals: false,
        mock_missing_tables: false,
        abi,
        show_progressbar: false,
        isa,
    });

    debug!("webassembly - creating instance");
    let instance = Instance::new(&module, import_object, options)?;
    debug!("webassembly - instance created");
    Ok(ResultObject { module, instance })
}

/// The webassembly::instantiate_streaming() function compiles and instantiates
/// a WebAssembly module directly from a streamed underlying source.
/// This is the most efficient, optimized way to load wasm code.
pub fn instantiate_streaming(
    _buffer_source: Vec<u8>,
    _import_object: ImportObject<&str, &str>,
) -> Result<ResultObject, ErrorKind> {
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
pub fn compile(buffer_source: Vec<u8>) -> Result<Module, ErrorKind> {
    // TODO: This should be automatically validated when creating the Module
    debug!("webassembly - validating module");
    validate_or_error(&buffer_source)?;

    let isa = get_isa();

    debug!("webassembly - creating module");
    let module = Module::from_bytes(buffer_source, isa.frontend_config())?;
    debug!("webassembly - module created");

    Ok(module)
}

/// The webassembly::validate() function validates a given typed
/// array of WebAssembly binary code, returning whether the bytes
/// form a valid wasm module (true) or not (false).
/// Params:
/// * `buffer_source`: A `&[u8]` containing the
///   binary code of the .wasm module you want to compile.
pub fn validate(buffer_source: &[u8]) -> bool {
    validate_or_error(buffer_source).is_ok()
}

pub fn validate_or_error(bytes: &[u8]) -> Result<(), ErrorKind> {
    let mut parser = wasmparser::ValidatingParser::new(bytes, None);
    loop {
        let state = parser.read();
        match *state {
            wasmparser::ParserState::EndWasm => return Ok(()),
            wasmparser::ParserState::Error(err) => {
                return Err(ErrorKind::CompileError(format!(
                    "Validation error: {}",
                    err.message
                )));
            }
            _ => (),
        }
    }
}

pub fn get_isa() -> Box<isa::TargetIsa> {
    let flags = {
        let mut builder = settings::builder();
        builder.set("opt_level", "best").unwrap();

        if cfg!(not(test)) {
            builder.set("enable_verifier", "false").unwrap();
        }

        let flags = settings::Flags::new(builder);
        debug_assert_eq!(flags.opt_level(), settings::OptLevel::Best);
        flags
    };
    isa::lookup(triple!("x86_64")).unwrap().finish(flags)
}

fn store_module_arguments(path: &str, args: Vec<&str>, instance: &Instance) -> (u32, u32) {
    let argc = args.len() + 1;

    let (argv_offset, argv_slice): (_, &mut [u32]) =
        unsafe { allocate_on_stack(((argc + 1) * 4) as u32, instance) };
    assert!(!argv_slice.is_empty());

    argv_slice[0] = unsafe { allocate_cstr_on_stack(path, instance).0 };

    for (slot, arg) in argv_slice[1..argc].iter_mut().zip(args.iter()) {
        *slot = unsafe { allocate_cstr_on_stack(&arg, instance).0 };
    }

    argv_slice[argc] = 0;

    (argc as u32, argv_offset)
}

// fn get_module_arguments(options: &Run, instance: &mut webassembly::Instance) -> (u32, u32) {
//     // Application Arguments
//     let mut arg_values: Vec<String> = Vec::new();
//     let mut arg_addrs: Vec<*const u8> = Vec::new();
//     let arg_length = options.args.len() + 1;

//     arg_values.reserve_exact(arg_length);
//     arg_addrs.reserve_exact(arg_length);

//     // Push name of wasm file
//     arg_values.push(format!("{}\0", options.path.to_str().unwrap()));
//     arg_addrs.push(arg_values[0].as_ptr());

//     // Push additional arguments
//     for (i, arg) in options.args.iter().enumerate() {
//         arg_values.push(format!("{}\0", arg));
//         arg_addrs.push(arg_values[i + 1].as_ptr());
//     }

//     // Get argument count and pointer to addresses
//     let argv = arg_addrs.as_ptr() as *mut *mut i8;
//     let argc = arg_length as u32;

//     // Copy the the arguments into the wasm memory and get offset
//     let argv_offset =  unsafe {
//         copy_cstr_array_into_wasm(argc, argv, instance)
//     };

//     debug!("argc = {:?}", argc);
//     debug!("argv = {:?}", arg_addrs);

//     (argc, argv_offset)
// }

pub fn start_instance(
    module: &Module,
    instance: &mut Instance,
    path: &str,
    args: Vec<&str>,
) -> Result<(), String> {
    if let Some(ref emscripten_data) = &instance.emscripten_data {
        emscripten_data.atinit(module, instance)?;

        let func_index = match module.info.exports.get("_main") {
            Some(&Export::Function(index)) => index,
            _ => panic!("_main emscripten function not found"),
        };

        let sig_index = module.get_func_type(func_index);
        let signature = module.get_signature(sig_index);
        let num_params = signature.params.len();
        let result = match num_params {
            2 => {
                let main: extern "C" fn(u32, u32, &Instance) =
                    get_instance_function!(instance, func_index);
                let (argc, argv) = store_module_arguments(path, args, instance);
                call_protected!(main(argc, argv, &instance))
            }
            0 => {
                let main: extern "C" fn(&Instance) = get_instance_function!(instance, func_index);
                call_protected!(main(&instance))
            }
            _ => panic!(
                "The emscripten main function has received an incorrect number of params {}",
                num_params
            ),
        }
        .map_err(|err| format!("{}", err));

        emscripten_data.atexit(module, instance)?;

        result
    } else {
        let func_index =
            instance
                .start_func
                .unwrap_or_else(|| match module.info.exports.get("main") {
                    Some(&Export::Function(index)) => index,
                    _ => panic!("Main function not found"),
                });
        let main: extern "C" fn(&Instance) = get_instance_function!(instance, func_index);
        call_protected!(main(&instance)).map_err(|err| format!("{}", err))
    }
}
