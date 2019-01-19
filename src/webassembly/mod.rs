pub mod libcalls;
pub mod relocation;
pub mod utils;

use wasmer_clif_backend::CraneliftCompiler;
use wasmer_runtime::{
    self as runtime,
    error::{CallResult, Result},
    import::Imports,
    instance::Instance,
    module::{Module, ModuleInner},
};

use cranelift_codegen::{
    isa,
    settings::{self, Configurable},
};
use std::panic;
use std::rc::Rc;
use std::str::FromStr;
use std::sync::Arc;
use target_lexicon;
use wasmparser;
use wasmparser::WasmDecoder;

use wasmer_emscripten::{allocate_cstr_on_stack, allocate_on_stack, is_emscripten_module};

pub struct ResultObject {
    /// A webassembly::Module object representing the compiled WebAssembly module.
    /// This Module can be instantiated again
    pub module: Module,
    /// A webassembly::Instance object that contains all the Exported WebAssembly
    /// functions.
    pub instance: Box<Instance>,
}

pub struct InstanceOptions {
    // Shall we mock automatically the imported functions if they don't exist?
    pub mock_missing_imports: bool,
    pub mock_missing_globals: bool,
    pub mock_missing_tables: bool,
    pub abi: InstanceABI,
    pub show_progressbar: bool,
    //    pub isa: Box<isa::TargetIsa>, TODO isa
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
pub fn instantiate(
    buffer_source: &[u8],
    import_object: &Imports,
    options: Option<InstanceOptions>,
) -> Result<ResultObject> {
    debug!("webassembly - creating instance");

    //let instance = Instance::new(&module, import_object, options)?;
    unimplemented!()
    //    let instance = wasmer_runtime::instantiate(buffer_source, &CraneliftCompiler::new(), import_object)
    //        .map_err(|e| ErrorKind::CompileError(e))?;
    //
    //    let isa = get_isa();
    //    let abi = if is_emscripten_module(&instance.module) {
    //        InstanceABI::Emscripten
    //    } else {
    //        InstanceABI::None
    //    };
    //
    //    let options = options.unwrap_or_else(|| InstanceOptions {
    //        mock_missing_imports: false,
    //        mock_missing_globals: false,
    //        mock_missing_tables: false,
    //        abi,
    //        show_progressbar: false,
    //        isa,
    //    });

    //    debug!("webassembly - instance created");
    //    Ok(ResultObject {
    //        module: Arc::clone(&instance.module),
    //        instance,
    //    })
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

pub fn start_instance(
    module: &Module,
    instance: &mut Instance,
    path: &str,
    args: Vec<&str>,
) -> CallResult<()> {
    let main_name = if is_emscripten_module(module) {
        "_main"
    } else {
        "main"
    };

    // TODO handle args
    instance.call(main_name, &[])?;
    // TODO atinit and atexit for emscripten

    Ok(())
}
