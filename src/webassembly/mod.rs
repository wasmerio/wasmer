pub mod module;
pub mod compilation;
pub mod memory;
pub mod environ;
pub mod instance;
pub mod errors;
pub mod execute;
pub mod utils;

use cranelift_native;
use cranelift_codegen::isa::TargetIsa;
use cranelift_codegen::settings;
use cranelift_codegen::settings::Configurable;

pub use self::compilation::compile_module;
pub use self::environ::ModuleEnvironment;
pub use self::module::Module;
pub use self::instance::Instance;
pub use self::errors::{Error, ErrorKind};
pub use self::execute::{compile_and_link_module,execute};
use wasmparser;

pub struct ResultObject {
    /// A WebAssembly.Module object representing the compiled WebAssembly module.
    /// This Module can be instantiated again
    pub module: Module,
    /// A WebAssembly.Instance object that contains all the Exported WebAssembly
    /// functions.
    pub instance: Instance
}

pub struct ImportObject {
}

/// The WebAssembly.instantiate() function allows you to compile and
/// instantiate WebAssembly code

/// Params: 
/// * `buffer_source`: A `Vec<u8>` containing the
///   binary code of the .wasm module you want to compile.

/// * `import_object`: An object containing the values to be imported
///   into the newly-created Instance, such as functions or
///   WebAssembly.Memory objects. There must be one matching property
///   for each declared import of the compiled module or else a
///   WebAssembly.LinkError is thrown.

/// Errors:
/// If the operation fails, the Result rejects with a 
/// WebAssembly.CompileError, WebAssembly.LinkError, or
///  WebAssembly.RuntimeError, depending on the cause of the failure.
pub fn instantiate(buffer_source: Vec<u8>, import_object: Option<ImportObject>) -> Result<ResultObject, Error> {
    let isa = construct_isa();
    println!("instantiate::init");
    let mut module = Module::new();
    let environ = ModuleEnvironment::new(&*isa, &mut module);
    let translation = environ.translate(&buffer_source).map_err(|e| ErrorKind::CompileError(e.to_string()))?;
    println!("instantiate::compile and link");
    let compilation = compile_and_link_module(&*isa, &translation)?;
    // let (compilation, relocations) = compile_module(&translation, &*isa)?;
    println!("instantiate::instantiate");

    let mut instance = Instance::new(
        translation.module,
        &compilation,
        &translation.lazy.data_initializers,
    );
    println!("instantiate::execute");

    let x = execute(&module, &compilation, &mut instance)?;

    // let instance = Instance {
    //     tables: Vec::new(),
    //     memories: Vec::new(),
    //     globals: Vec::new(),
    // };

    Ok(ResultObject {
        module,
        instance
    })
}

/// The WebAssembly.compile() function compiles a WebAssembly.Module
/// from WebAssembly binary code.  This function is useful if it
/// is necessary to a compile a module before it can be instantiated
/// (otherwise, the WebAssembly.instantiate() function should be used).

/// Params: 
/// * `buffer_source`: A `Vec<u8>` containing the
///   binary code of the .wasm module you want to compile.

/// Errors:
/// If the operation fails, the Result rejects with a 
/// WebAssembly.CompileError.
pub fn compile(buffer_source: Vec<u8>) -> Result<Module, Error> {
    let isa = construct_isa();

    let mut module = Module::new();
    let environ = ModuleEnvironment::new(&*isa, &mut module);
    let translation = environ.translate(&buffer_source).map_err(|e| ErrorKind::CompileError(e.to_string()))?;
    // compile_module(&translation, &*isa)?;
    compile_and_link_module(&*isa, &translation)?;

    Ok(module)
}

fn construct_isa() -> Box<TargetIsa> {
    let (mut flag_builder, isa_builder) = cranelift_native::builders().unwrap_or_else(|_| {
        panic!("host machine is not a supported target");
    });

    // Enable verifier passes in debug mode.
    // if cfg!(debug_assertions) {
    flag_builder.enable("enable_verifier").unwrap();
    // }

    // Enable optimization if requested.
    // if args.flag_optimize {
    flag_builder.set("opt_level", "best").unwrap();
    // }

    isa_builder.finish(settings::Flags::new(flag_builder))
}

/// The WebAssembly.validate() function validates a given typed
/// array of WebAssembly binary code, returning whether the bytes
/// form a valid wasm module (true) or not (false).

/// Params: 
/// * `buffer_source`: A `Vec<u8>` containing the
///   binary code of the .wasm module you want to compile.
pub fn validate(buffer_source: &Vec<u8>) -> bool {
    wasmparser::validate(buffer_source, None)
}
