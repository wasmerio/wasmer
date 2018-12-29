// cranelift
use cranelift_codegen::cursor::FuncCursor;
use cranelift_codegen::ir::immediates::{Offset32, Uimm64};
use cranelift_codegen::ir::types::{self, *};
use cranelift_codegen::ir::{
    self, AbiParam, ArgumentPurpose, ExtFuncData, ExternalName, FuncRef, InstBuilder, Signature,
    TrapCode,
};
use cranelift_codegen::isa::{self, CallConv, TargetFrontendConfig};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_entity::{EntityRef, PrimaryMap};
use target_lexicon;
use cranelift_wasm::{
    translate_module, DefinedFuncIndex, FuncEnvironment as FuncEnvironmentTrait, FuncIndex,
    FuncTranslator, Global, GlobalIndex, GlobalVariable, Memory, MemoryIndex, ModuleEnvironment,
    ReturnMode, SignatureIndex, Table, TableIndex, WasmResult,
};

// webassembly
use crate::webassembly::errors::{ErrorKind};

// runtime
use crate::runtime::{
    module::{Module},
    types::{SigIndex, Type},
    vm::{Ctx},
};

///
pub trait CraneliftModuleTrait {
    fn from_bytes(buffer_source: Vec<u8>) -> Module;
    fn func_env(&self) -> FuncEnvironment;
}

///
impl CraneliftModuleTrait for Module {
    fn from_bytes(buffer_source: Vec<u8>) -> Module {
        let mut module = Module::new();
        //
        translate_module(&buffer_source, &mut module)
            .map_err(|e| ErrorKind::CompileError(e.to_string()))?;

        module
    }

    //
    fn func_env(&self) -> FuncEnvironment {
        FuncEnvironment::new(&self)
    }
}

///
pub struct FuncEnvironment<'environment> {
    pub module: &'environment Module,
    pub isa: Box<isa::TargetIsa>,
}

///
impl<'environment> FuncEnvironment<'environment> {
    pub fn new(module: &'environment Module) -> Self {
        let isa = isa.frontend_config();
        Self {
            module,
            isa,
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

        // TODO: Make portable.
        isa::lookup(triple!("x86_64")).unwrap().finish(flags)
    }

    pub fn get_type(&self, ty: Type) -> types::Type {
        match ty {
            Type::I32 => I32,
            Type::I64 => I64,
            Type::F32 => F32,
            Type::F64 => F64,
        }
    }

    /// Creates a signature with VMContext as the last param
    pub fn generate_sig(&self, sigidx: SigIndex) -> ir::Signature {
        let mut sig = Signature::new(self.isa.default_call_conv());

        let signature = self.module.signatures[sigidx];

        // Generate the parameters
        for param in signature.params {
            let ty = self.get_type(param);
            sig.params.push(AbiParam::new(ty));
        }

        // Generate return types
        for ret in signature.returns {
            let ty = self.get_type(ret);
            sig.returns.push(AbiParam::new(ty));
        }

        // Finally add the VMContext parameter
        sig.params.push(AbiParam::special(self.pointer_type(), ir::ArgumentPurpose::VMContext));

        sig
    }
}

/// 
impl<'environment> FuncEnvironmentTrait for FuncEnvironment<'environment> {
    /// Gets configuration information needed for compiling functions
    fn target_config(&self) -> TargetFrontendConfig {
        self.isa.frontend_config()
    }

    /// Gets native pointers types.
    /// `I64` on 64-bit arch; `I32` on 32-bit arch.
    fn pointer_type(&self) -> ir::Type {
        ir::Type::int(u16::from(self.isa.target_config().pointer_bits())).unwrap()
    }

    /// Get the size of a native pointer in bytes.
    fn pointer_bytes(&self) -> u8 {
        self.isa.target_config().pointer_bytes()
    }
}
