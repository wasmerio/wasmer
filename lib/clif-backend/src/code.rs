// Parts of the following code are Copyright 2018 Cranelift Developers
// and subject to the license https://github.com/CraneStation/cranelift/blob/c47ca7bafc8fc48358f1baa72360e61fc1f7a0f2/cranelift-wasm/LICENSE

use crate::{func_env::FuncEnv, module::{Converter, Module}, signal::Caller, get_isa};
use std::sync::Arc;
use wasmer_runtime_core::{
    backend::{Backend, CacheGen, Token},
    cache::{Artifact, Error as CacheError},
    codegen::*,
    module::{ModuleInfo, ModuleInner},
    structures::{TypedIndex, Map},
    types::{
        FuncSig, FuncIndex,
        ElementType, GlobalDescriptor, GlobalIndex, GlobalInit, Initializer, LocalFuncIndex,
        LocalOrImport, MemoryDescriptor, SigIndex, TableDescriptor, Value,
    },
};
use cranelift_codegen::isa;
use cranelift_codegen::entity::EntityRef;
use cranelift_codegen::ir::{self, Ebb, InstBuilder, ValueLabel};
use cranelift_codegen::timing;
use wasmparser::Type as WpType;
use cranelift_wasm::{translate_operator, TranslationState, get_vmctx_value_label};
use cranelift_wasm::{self, translate_module, FuncTranslator, ModuleEnvironment};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};

pub struct CraneliftModuleCodeGenerator {
    isa: Box<isa::TargetIsa>,
    signatures: Option<Arc<Map<SigIndex, FuncSig>>>,
    pub clif_signatures: Map<SigIndex, ir::Signature>,
    function_signatures: Option<Arc<Map<FuncIndex, SigIndex>>>,
    functions: Vec<CraneliftFunctionCodeGenerator>,
    func_bodies: Map<LocalFuncIndex, ir::Function>,
}

pub struct ClifFuncEnv {

}

impl ModuleCodeGenerator<CraneliftFunctionCodeGenerator, Caller, CodegenError>
    for CraneliftModuleCodeGenerator
{
    fn new() -> Self {
        let isa = get_isa();
        CraneliftModuleCodeGenerator {
            isa,
            clif_signatures: Map::new(), // TODO FIX
            functions: vec![],
            function_signatures: None,
            signatures: None,
            func_bodies: Map::new(),
        }
    }

    fn backend_id() -> Backend {
        Backend::Cranelift
    }

    fn check_precondition(&mut self, _module_info: &ModuleInfo) -> Result<(), CodegenError> {
        Ok(())
    }

    fn next_function(&mut self, module_info: &ModuleInfo) -> Result<&mut CraneliftFunctionCodeGenerator, CodegenError> {

        // define_function_body(




        let mut func_translator = FuncTranslator::new();

//        let func_body = {


//            let mut func_env = FuncEnv::new(self);

            let func_index = self.func_bodies.next_index();
            let name = ir::ExternalName::user(0, func_index.index() as u32);

            let sig = generate_signature(self,
                self.get_func_type(&module_info, Converter(func_index.convert_up(&module_info)).into()),
            );

            let mut func = ir::Function::with_name_signature(name, sig);

            //func_translator.translate(body_bytes, body_offset, &mut func, &mut func_env)?;
            // This clears the `FunctionBuilderContext`.

            let mut func_env = CraneliftFunctionCodeGenerator {
                builder: None,
                func_body: func,
                func_translator,
    //            translator:
            };
            let builder = FunctionBuilder::new(&mut func_env.func_body, &mut func_env.func_translator.func_ctx);
            func_env.builder = Some(builder);

            let mut builder = func_env.builder.as_ref().unwrap();

            // TODO srcloc
            //builder.set_srcloc(cur_srcloc(&reader));

            let entry_block = builder.create_ebb();
            builder.append_ebb_params_for_function_params(entry_block);
            builder.switch_to_block(entry_block); // This also creates values for the arguments.
            builder.seal_block(entry_block);
            // Make sure the entry block is inserted in the layout before we make any callbacks to
            // `environ`. The callback functions may need to insert things in the entry block.
            builder.ensure_inserted_ebb();

            let num_params = declare_wasm_parameters(&mut builder, entry_block);

            // Set up the translation state with a single pushed control block representing the whole
            // function and its return values.
            let exit_block = builder.create_ebb();
            builder.append_ebb_params_for_function_returns(exit_block);
            func_translator.state.initialize(&builder.func.signature, exit_block);


            #[cfg(feature = "debug")]
                {
                    use cranelift_codegen::cursor::{Cursor, FuncCursor};
                    use cranelift_codegen::ir::InstBuilder;
                    let entry_ebb = func.layout.entry_block().unwrap();
                    let ebb = func.dfg.make_ebb();
                    func.layout.insert_ebb(ebb, entry_ebb);
                    let mut pos = FuncCursor::new(&mut func).at_first_insertion_point(ebb);
                    let params = pos.func.dfg.ebb_params(entry_ebb).to_vec();

                    let new_ebb_params: Vec<_> = params
                        .iter()
                        .map(|&param| {
                            pos.func
                                .dfg
                                .append_ebb_param(ebb, pos.func.dfg.value_type(param))
                        })
                        .collect();

                    let start_debug = {
                        let signature = pos.func.import_signature(ir::Signature {
                            call_conv: self.target_config().default_call_conv,
                            params: vec![
                                ir::AbiParam::special(ir::types::I64, ir::ArgumentPurpose::VMContext),
                                ir::AbiParam::new(ir::types::I32),
                            ],
                            returns: vec![],
                        });

                        let name = ir::ExternalName::testcase("strtdbug");

                        pos.func.import_function(ir::ExtFuncData {
                            name,
                            signature,
                            colocated: false,
                        })
                    };

                    let end_debug = {
                        let signature = pos.func.import_signature(ir::Signature {
                            call_conv: self.target_config().default_call_conv,
                            params: vec![ir::AbiParam::special(
                                ir::types::I64,
                                ir::ArgumentPurpose::VMContext,
                            )],
                            returns: vec![],
                        });

                        let name = ir::ExternalName::testcase("enddbug");

                        pos.func.import_function(ir::ExtFuncData {
                            name,
                            signature,
                            colocated: false,
                        })
                    };

                    let i32_print = {
                        let signature = pos.func.import_signature(ir::Signature {
                            call_conv: self.target_config().default_call_conv,
                            params: vec![
                                ir::AbiParam::special(ir::types::I64, ir::ArgumentPurpose::VMContext),
                                ir::AbiParam::new(ir::types::I32),
                            ],
                            returns: vec![],
                        });

                        let name = ir::ExternalName::testcase("i32print");

                        pos.func.import_function(ir::ExtFuncData {
                            name,
                            signature,
                            colocated: false,
                        })
                    };

                    let i64_print = {
                        let signature = pos.func.import_signature(ir::Signature {
                            call_conv: self.target_config().default_call_conv,
                            params: vec![
                                ir::AbiParam::special(ir::types::I64, ir::ArgumentPurpose::VMContext),
                                ir::AbiParam::new(ir::types::I64),
                            ],
                            returns: vec![],
                        });

                        let name = ir::ExternalName::testcase("i64print");

                        pos.func.import_function(ir::ExtFuncData {
                            name,
                            signature,
                            colocated: false,
                        })
                    };

                    let f32_print = {
                        let signature = pos.func.import_signature(ir::Signature {
                            call_conv: self.target_config().default_call_conv,
                            params: vec![
                                ir::AbiParam::special(ir::types::I64, ir::ArgumentPurpose::VMContext),
                                ir::AbiParam::new(ir::types::F32),
                            ],
                            returns: vec![],
                        });

                        let name = ir::ExternalName::testcase("f32print");

                        pos.func.import_function(ir::ExtFuncData {
                            name,
                            signature,
                            colocated: false,
                        })
                    };

                    let f64_print = {
                        let signature = pos.func.import_signature(ir::Signature {
                            call_conv: self.target_config().default_call_conv,
                            params: vec![
                                ir::AbiParam::special(ir::types::I64, ir::ArgumentPurpose::VMContext),
                                ir::AbiParam::new(ir::types::F64),
                            ],
                            returns: vec![],
                        });

                        let name = ir::ExternalName::testcase("f64print");

                        pos.func.import_function(ir::ExtFuncData {
                            name,
                            signature,
                            colocated: false,
                        })
                    };

                    let vmctx = pos
                        .func
                        .special_param(ir::ArgumentPurpose::VMContext)
                        .expect("missing vmctx parameter");

                    let func_index = pos.ins().iconst(
                        ir::types::I32,
                        func_index.index() as i64 + self.module.info.imported_functions.len() as i64,
                    );

                    pos.ins().call(start_debug, &[vmctx, func_index]);

                    for param in new_ebb_params.iter().cloned() {
                        match pos.func.dfg.value_type(param) {
                            ir::types::I32 => pos.ins().call(i32_print, &[vmctx, param]),
                            ir::types::I64 => pos.ins().call(i64_print, &[vmctx, param]),
                            ir::types::F32 => pos.ins().call(f32_print, &[vmctx, param]),
                            ir::types::F64 => pos.ins().call(f64_print, &[vmctx, param]),
                            _ => unimplemented!(),
                        };
                    }

                    pos.ins().call(end_debug, &[vmctx]);

                    pos.ins().jump(entry_ebb, new_ebb_params.as_slice());
                }

//            func
//        };

        // Add function body to list of function bodies.
//        self.func_bodies.push(func);




        self.functions.push(func_env);
        Ok(self.functions.last_mut().unwrap())
    }

    fn finalize(
        self,
        _module_info: &ModuleInfo,
    ) -> Result<(Caller, Box<dyn CacheGen>), CodegenError> {
        unimplemented!()
    }

    fn feed_signatures(&mut self, signatures: Map<SigIndex, FuncSig>) -> Result<(), CodegenError> {
        self.signatures = Some(Arc::new(signatures));
        Ok(())
    }

    fn feed_function_signatures(
        &mut self,
        assoc: Map<FuncIndex, SigIndex>,
    ) -> Result<(), CodegenError> {
        self.function_signatures = Some(Arc::new(assoc));
        Ok(())
    }

    fn feed_import_function(&mut self) -> Result<(), CodegenError> {
        Ok(())
    }

    unsafe fn from_cache(_cache: Artifact, _: Token) -> Result<ModuleInner, CacheError> {
        unimplemented!()
    }
}

pub struct CraneliftFunctionCodeGenerator {
    func_body: ir::Function,
    builder: Option<FunctionBuilder<'static>>,
    func_translator: FuncTranslator,
}

impl FunctionCodeGenerator<CodegenError> for CraneliftFunctionCodeGenerator {
    fn feed_return(&mut self, _ty: WpType) -> Result<(), CodegenError> {
        Ok(())
    }

    fn feed_param(&mut self, _ty: WpType) -> Result<(), CodegenError> {
        Ok(())
    }

    fn feed_local(&mut self, _ty: WpType, _n: usize) -> Result<(), CodegenError> {
        Ok(())
    }

    fn begin_body(&mut self, _module_info: &ModuleInfo) -> Result<(), CodegenError> {
        Ok(())
    }

    fn feed_event(&mut self, event: Event, _module_info: &ModuleInfo) -> Result<(), CodegenError> {
        let op = match event {
            Event::Wasm(x) => x,
            Event::Internal(_x) => {
                return Ok(());
            }
        };
        let builder = &self.builder;
        //let func_environment = FuncEnv::new();
        //let state = TranslationState::new();
        //translate_operator(*op, builder, state, func_environment);
        Ok(())
    }

    fn finalize(&mut self) -> Result<(), CodegenError> {
        self.builder.as_mut().unwrap().finalize();
        Ok(())
    }
}

#[derive(Debug)]
pub struct CodegenError {
    pub message: String,
}

impl CraneliftModuleCodeGenerator {
    /// Return the signature index for the given function index.
    pub fn get_func_type(
        &self,
        module_info: &ModuleInfo,
        func_index: cranelift_wasm::FuncIndex,
    ) -> cranelift_wasm::SignatureIndex {
        let sig_index: SigIndex = module_info.func_assoc[Converter(func_index).into()];
        Converter(sig_index).into()
    }



}

/// Creates a signature with VMContext as the last param
fn generate_signature(
    env: &CraneliftModuleCodeGenerator,
    clif_sig_index: cranelift_wasm::SignatureIndex,
) -> ir::Signature {
    // Get signature
    let mut signature = env.clif_signatures[Converter(clif_sig_index).into()].clone();

    // Add the vmctx parameter type to it
    signature.params.insert(
        0,
        ir::AbiParam::special(pointer_type(env), ir::ArgumentPurpose::VMContext),
    );

    // Return signature
    signature
}

fn pointer_type(mcg: &CraneliftModuleCodeGenerator) -> ir::Type {
    ir::Type::int(u16::from(mcg.isa.frontend_config().pointer_bits())).unwrap()
}


/// Declare local variables for the signature parameters that correspond to WebAssembly locals.
///
/// Return the number of local variables declared.
fn declare_wasm_parameters(builder: &mut FunctionBuilder, entry_block: Ebb) -> usize {
    let sig_len = builder.func.signature.params.len();
    let mut next_local = 0;
    for i in 0..sig_len {
        let param_type = builder.func.signature.params[i];
        // There may be additional special-purpose parameters following the normal WebAssembly
        // signature parameters. For example, a `vmctx` pointer.
        if param_type.purpose == ir::ArgumentPurpose::Normal {
            // This is a normal WebAssembly signature parameter, so create a local for it.
            let local = Variable::new(next_local);
            builder.declare_var(local, param_type.value_type);
            next_local += 1;

            let param_value = builder.ebb_params(entry_block)[i];
            builder.def_var(local, param_value);
        }
        if param_type.purpose == ir::ArgumentPurpose::VMContext {
            let param_value = builder.ebb_params(entry_block)[i];
            builder.set_val_label(param_value, get_vmctx_value_label());
        }
    }

    next_local
}

