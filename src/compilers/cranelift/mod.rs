use crate::runtime::Module;
use std::sync::Arc;
use crate::webassembly;
use crate::runtime::Compiler;
use cranelift_wasm::{
    translate_module, DefinedFuncIndex, FuncEnvironment as FuncEnvironmentTrait, FuncIndex,
    FuncTranslator, Global, GlobalIndex, GlobalVariable, Memory, MemoryIndex, ModuleEnvironment,
    ReturnMode, SignatureIndex, Table, TableIndex, WasmResult,
};
use cranelift_codegen::isa::{CallConv, TargetFrontendConfig};

use crate::webassembly::{Error, ErrorKind};

pub struct CraneliftCompiler {}

impl Compiler for CraneliftCompiler {

    fn compile(&self, wasm: &[u8]) -> Result<Arc<Module>, String> {
        debug!("webassembly - validating module");
        // TODO: This should be automatically validated when creating the Module
        webassembly::validate_or_error(&wasm).map_err(|err| format!("{}", err))?;

        let isa = webassembly::get_isa();

        debug!("webassembly - creating module");

        // TODO Implement Module with ModuleEnvironment
//        let mut module = Module {
//
//        };
//        translate_module(&buffer_source, &mut module)
//            .map_err(|e| ErrorKind::CompileError(e.to_string()))?;

        debug!("webassembly - module created");
        unimplemented!()
    }

}
