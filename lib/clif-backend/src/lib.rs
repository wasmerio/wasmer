mod cache;
mod func_env;
mod libcalls;
mod module;
mod module_env;
mod relocation;
mod resolver;
mod signal;
mod trampoline;

use cranelift_codegen::{
    isa,
    settings::{self, Configurable},
};
use target_lexicon::Triple;

use wasmer_runtime_core::cache::{Artifact, Error as CacheError};
use wasmer_runtime_core::{
    backend::{Compiler, CompilerConfig, Token},
    error::{CompileError, CompileResult},
    module::ModuleInner,
};

#[macro_use]
extern crate serde_derive;

extern crate rayon;
extern crate serde;

use wasmparser::{self, WasmDecoder};

pub struct CraneliftCompiler {}

impl CraneliftCompiler {
    pub fn new() -> Self {
        Self {}
    }
}

impl Compiler for CraneliftCompiler {
    /// Compiles wasm binary to a wasmer module.
    fn compile(
        &self,
        wasm: &[u8],
        compiler_config: CompilerConfig,
        _: Token,
    ) -> CompileResult<ModuleInner> {
        validate(wasm)?;

        let isa = get_isa();

        let mut module = module::Module::new(&compiler_config);
        let module_env = module_env::ModuleEnv::new(&mut module, &*isa);

        let func_bodies = module_env.translate(wasm)?;

        module.compile(&*isa, func_bodies)
    }

    /// Create a wasmer Module from an already-compiled cache.

    unsafe fn from_cache(&self, cache: Artifact, _: Token) -> Result<ModuleInner, CacheError> {
        module::Module::from_cache(cache)
    }

    //
    // fn compile_to_backend_cache_data(
    //     &self,
    //     wasm: &[u8],
    //     _: Token,
    // ) -> CompileResult<(Box<ModuleInfo>, Vec<u8>, Memory)> {
    //     validate(wasm)?;

    //     let isa = get_isa();

    //     let mut module = module::Module::new(wasm);
    //     let module_env = module_env::ModuleEnv::new(&mut module, &*isa);

    //     let func_bodies = module_env.translate(wasm)?;

    //     let (info, backend_cache, compiled_code) = module
    //         .compile_to_backend_cache(&*isa, func_bodies)
    //         .map_err(|e| CompileError::InternalError {
    //             msg: format!("{:?}", e),
    //         })?;

    //     let buffer =
    //         backend_cache
    //             .into_backend_data()
    //             .map_err(|e| CompileError::InternalError {
    //                 msg: format!("{:?}", e),
    //             })?;

    //     Ok((Box::new(info), buffer, compiled_code))
    // }
}

fn get_isa() -> Box<isa::TargetIsa> {
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
    isa::lookup(Triple::host()).unwrap().finish(flags)
}

fn validate(bytes: &[u8]) -> CompileResult<()> {
    let mut parser = wasmparser::ValidatingParser::new(bytes, None);
    loop {
        let state = parser.read();
        match *state {
            wasmparser::ParserState::EndWasm => break Ok(()),
            wasmparser::ParserState::Error(err) => Err(CompileError::ValidationError {
                msg: err.message.to_string(),
            })?,
            _ => {}
        }
    }
}

/// The current version of this crate
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
