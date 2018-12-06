pub mod errors;
pub mod import_object;
pub mod instance;
pub mod math_intrinsics;
pub mod memory;
pub mod module;
pub mod relocation;
pub mod utils;

use cranelift_codegen::{
    isa,
    settings::{self, Configurable},
};
use std::panic;
use std::str::FromStr;
use target_lexicon;
use wasmparser;
use wasmparser::WasmDecoder;

pub use self::errors::{Error, ErrorKind};
pub use self::import_object::{ImportObject, ImportValue};
pub use self::instance::{Instance, InstanceOptions};
pub use self::memory::LinearMemory;
pub use self::module::{Export, Module, ModuleInfo};
use crate::apis::is_emscripten_module;

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
) -> Result<ResultObject, ErrorKind> {
    let flags = {
        let mut builder = settings::builder();
        builder.set("opt_level", "best").unwrap();

        let flags = settings::Flags::new(builder);
        debug_assert_eq!(flags.opt_level(), settings::OptLevel::Best);
        flags
    };
    let isa = isa::lookup(triple!("x86_64")).unwrap().finish(flags);

    let module = compile(buffer_source)?;
    debug!("webassembly - creating instance");
    let instance = Instance::new(
        &module,
        import_object,
        InstanceOptions {
            mock_missing_imports: true,
            mock_missing_globals: true,
            mock_missing_tables: true,
            use_emscripten: is_emscripten_module(&module),
            show_progressbar: true,
            isa: isa,
        },
    )?;
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

    let flags = settings::Flags::new(settings::builder());
    let isa = isa::lookup(triple!("x86_64")).unwrap().finish(flags);

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
                )))
            }
            _ => (),
        }
    }
}
