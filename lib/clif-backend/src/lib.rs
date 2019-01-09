mod codegen;
mod libcalls;
mod relocation;
mod resolver;

use cranelift_codegen::{
    isa,
    settings::{self, Configurable},
};
use target_lexicon::Triple;
use wasmer_runtime::{Compiler, Module};
use wasmparser::{self, WasmDecoder};

use self::codegen::converter;
use self::codegen::CraneliftModule;

pub struct CraneliftCompiler {}

impl CraneliftCompiler {
    pub fn new() -> Self {
        Self {}
    }
}

impl Compiler for CraneliftCompiler {
    // Compiles wasm binary to a wasmer module
    fn compile(&self, wasm: &[u8]) -> Result<Module, String> {
        validate(wasm)?;

        let isa = get_isa();
        // Generate a Cranlift module from wasm binary
        let cranelift_module = CraneliftModule::from_bytes(&wasm.to_vec(), isa.frontend_config())?;

        // Convert Cranelift module to wasmer module
        let wasmer_module = converter::convert_module(cranelift_module);

        // Return new wasmer module
        Ok(Module::new(wasmer_module))
    }
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

fn validate(bytes: &[u8]) -> Result<(), String> {
    let mut parser = wasmparser::ValidatingParser::new(bytes, None);
    loop {
        let state = parser.read();
        match *state {
            wasmparser::ParserState::EndWasm => return Ok(()),
            wasmparser::ParserState::Error(err) => {
                return Err(format!("Validation error: {}", err.message));
            }
            _ => (),
        }
    }
}
