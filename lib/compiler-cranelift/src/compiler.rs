//! Support for compiling with Cranelift.

use crate::address_map::get_function_address_map;
use crate::config::CraneliftConfig;
use crate::func_environ::{get_func_name, FuncEnvironment};
use crate::sink::{RelocSink, TrapSink};
use crate::trampoline::{
    make_trampoline_dynamic_function, make_trampoline_function_call, FunctionBuilderContext,
};
use crate::translator::{
    compiled_function_unwind_info, signature_to_cranelift_ir, transform_jump_table, FuncTranslator,
};
use cranelift_codegen::ir;
use cranelift_codegen::print_errors::pretty_error;
use cranelift_codegen::{binemit, isa, Context};
use rayon::prelude::{IntoParallelRefIterator, ParallelIterator};
use wasm_common::entity::PrimaryMap;
use wasm_common::{
    Features, FunctionIndex, FunctionType, LocalFunctionIndex, MemoryIndex, SignatureIndex,
    TableIndex,
};
use wasmer_compiler::CompileError;
use wasmer_compiler::{
    Compilation, CompiledFunction, CompiledFunctionFrameInfo, Compiler, FunctionBody,
    FunctionBodyData,
};
use wasmer_compiler::{CompilerConfig, ModuleTranslationState, Target};
use wasmer_runtime::{MemoryPlan, ModuleInfo, TablePlan};

/// A compiler that compiles a WebAssembly module with Cranelift, translating the Wasm to Cranelift IR,
/// optimizing it and then translating to assembly.
pub struct CraneliftCompiler {
    isa: Box<dyn isa::TargetIsa>,
    config: CraneliftConfig,
}

impl CraneliftCompiler {
    /// Creates a new Cranelift compiler
    pub fn new(config: &CraneliftConfig) -> Self {
        let isa = config.isa();
        Self {
            isa,
            config: config.clone(),
        }
    }

    /// Retrieves the starget ISA
    fn isa(&self) -> &dyn isa::TargetIsa {
        &*self.isa
    }

    /// Gets the WebAssembly features for this Compiler
    pub fn config(&self) -> &CraneliftConfig {
        &self.config
    }
}

impl Compiler for CraneliftCompiler {
    /// Gets the WebAssembly features for this Compiler
    fn features(&self) -> &Features {
        self.config.features()
    }

    /// Gets the target associated to the Cranelift ISA.
    fn target(&self) -> &Target {
        self.config.target()
    }

    /// Compile the module using Cranelift, producing a compilation result with
    /// associated relocations.
    fn compile_module(
        &self,
        module: &ModuleInfo,
        module_translation: &ModuleTranslationState,
        function_body_inputs: PrimaryMap<LocalFunctionIndex, FunctionBodyData<'_>>,
        memory_plans: PrimaryMap<MemoryIndex, MemoryPlan>,
        table_plans: PrimaryMap<TableIndex, TablePlan>,
    ) -> Result<Compilation, CompileError> {
        let isa = self.isa();
        let frontend_config = isa.frontend_config();
        let signatures = module
            .signatures
            .iter()
            .map(|(_sig_index, func_type)| signature_to_cranelift_ir(func_type, frontend_config))
            .collect::<PrimaryMap<SignatureIndex, ir::Signature>>();

        let functions = function_body_inputs
            .into_iter()
            .collect::<Vec<(LocalFunctionIndex, &FunctionBodyData<'_>)>>()
            .par_iter()
            .map_init(FuncTranslator::new, |func_translator, (i, input)| {
                let func_index = module.func_index(*i);
                let mut context = Context::new();
                let mut func_env = FuncEnvironment::new(
                    isa.frontend_config(),
                    module,
                    &signatures,
                    &memory_plans,
                    &table_plans,
                );
                context.func.name = get_func_name(func_index);
                context.func.signature = signatures[module.functions[func_index]].clone();
                context.func.collect_frame_layout_info();
                // if generate_debug_info {
                //     context.func.collect_debug_info();
                // }

                func_translator.translate(
                    module_translation,
                    input.data,
                    input.module_offset,
                    &mut context.func,
                    &mut func_env,
                )?;

                let mut code_buf: Vec<u8> = Vec::new();
                let mut reloc_sink = RelocSink::new(module, func_index);
                let mut trap_sink = TrapSink::new();
                let mut stackmap_sink = binemit::NullStackmapSink {};
                context
                    .compile_and_emit(
                        isa,
                        &mut code_buf,
                        &mut reloc_sink,
                        &mut trap_sink,
                        &mut stackmap_sink,
                    )
                    .map_err(|error| {
                        CompileError::Codegen(pretty_error(&context.func, Some(isa), error))
                    })?;

                let unwind_info = compiled_function_unwind_info(isa, &context);

                let address_map = get_function_address_map(&context, input, code_buf.len(), isa);

                // We transform the Cranelift JumpTable's into compiler JumpTables
                let func_jt_offsets = transform_jump_table(context.func.jt_offsets);

                Ok(CompiledFunction {
                    body: FunctionBody {
                        body: code_buf,
                        unwind_info,
                    },
                    jt_offsets: func_jt_offsets,
                    relocations: reloc_sink.func_relocs,
                    frame_info: CompiledFunctionFrameInfo {
                        address_map,
                        traps: trap_sink.traps,
                    },
                })
            })
            .collect::<Result<Vec<_>, CompileError>>()?
            .into_iter()
            .collect::<PrimaryMap<LocalFunctionIndex, _>>();

        let custom_sections = PrimaryMap::new();

        Ok(Compilation::new(functions, custom_sections))
    }

    fn compile_function_call_trampolines(
        &self,
        signatures: &[FunctionType],
    ) -> Result<Vec<FunctionBody>, CompileError> {
        signatures
            .par_iter()
            .map_init(FunctionBuilderContext::new, |mut cx, sig| {
                make_trampoline_function_call(&*self.isa, &mut cx, sig)
            })
            .collect::<Result<Vec<_>, CompileError>>()
    }

    fn compile_dynamic_function_trampolines(
        &self,
        signatures: &[FunctionType],
    ) -> Result<PrimaryMap<FunctionIndex, FunctionBody>, CompileError> {
        use wasmer_runtime::VMOffsets;
        let isa = self.isa();
        let frontend_config = isa.frontend_config();
        let offsets = VMOffsets::new_for_trampolines(frontend_config.pointer_bytes());
        Ok(signatures
            .par_iter()
            .map_init(FunctionBuilderContext::new, |mut cx, func_type| {
                make_trampoline_dynamic_function(&*self.isa, &offsets, &mut cx, &func_type)
            })
            .collect::<Result<Vec<_>, CompileError>>()?
            .into_iter()
            .collect::<PrimaryMap<FunctionIndex, FunctionBody>>())
    }
}
