//! Support for compiling with Cranelift.

use crate::address_map::get_function_address_map;
use crate::config::CraneliftConfig;
use crate::func_environ::{get_func_name, FuncEnvironment};
use crate::trampoline::{make_wasm_trampoline, FunctionBuilderContext};
use crate::translator::{
    irlibcall_to_libcall, irreloc_to_relocationkind, signature_to_cranelift_ir, FuncTranslator,
};
use crate::unwind::compiled_function_unwind_info;
use cranelift_codegen::ir::{self, ExternalName};
use cranelift_codegen::print_errors::pretty_error;
use cranelift_codegen::{binemit, isa, Context};
use rayon::prelude::{IntoParallelRefIterator, ParallelIterator};
use std::collections::HashMap;
use wasm_common::entity::{EntityRef, PrimaryMap, SecondaryMap};
use wasm_common::{
    DefinedFuncIndex, Features, FuncIndex, FuncType, MemoryIndex, SignatureIndex, SourceLoc,
    TableIndex,
};
use wasmer_compiler::CompileError;
use wasmer_compiler::FunctionBodyData;
use wasmer_compiler::{Compilation, CompiledFunction, Compiler, JumpTable};
use wasmer_compiler::{CompilerConfig, ModuleTranslationState, Target};
use wasmer_compiler::{Relocation, RelocationTarget};
use wasmer_runtime::{MemoryPlan, Module, TablePlan};
use wasmer_runtime::{TrapCode, TrapInformation};

/// Implementation of a relocation sink that just saves all the information for later
pub struct RelocSink {
    /// Current function index.
    func_index: FuncIndex,

    /// Relocations recorded for the function.
    pub func_relocs: Vec<Relocation>,
}

impl binemit::RelocSink for RelocSink {
    fn reloc_block(
        &mut self,
        _offset: binemit::CodeOffset,
        _reloc: binemit::Reloc,
        _block_offset: binemit::CodeOffset,
    ) {
        // This should use the `offsets` field of `ir::Function`.
        panic!("block headers not yet implemented");
    }
    fn reloc_external(
        &mut self,
        offset: binemit::CodeOffset,
        reloc: binemit::Reloc,
        name: &ExternalName,
        addend: binemit::Addend,
    ) {
        let reloc_target = if let ExternalName::User { namespace, index } = *name {
            debug_assert_eq!(namespace, 0);
            RelocationTarget::UserFunc(FuncIndex::from_u32(index))
        } else if let ExternalName::LibCall(libcall) = *name {
            RelocationTarget::LibCall(irlibcall_to_libcall(libcall))
        } else {
            panic!("unrecognized external name")
        };
        self.func_relocs.push(Relocation {
            kind: irreloc_to_relocationkind(reloc),
            reloc_target,
            offset,
            addend,
        });
    }

    fn reloc_constant(
        &mut self,
        _code_offset: binemit::CodeOffset,
        _reloc: binemit::Reloc,
        _constant_offset: ir::ConstantOffset,
    ) {
        // Do nothing for now: cranelift emits constant data after the function code and also emits
        // function code with correct relative offsets to the constant data.
    }

    fn reloc_jt(&mut self, offset: binemit::CodeOffset, reloc: binemit::Reloc, jt: ir::JumpTable) {
        self.func_relocs.push(Relocation {
            kind: irreloc_to_relocationkind(reloc),
            reloc_target: RelocationTarget::JumpTable(self.func_index, JumpTable::new(jt.index())),
            offset,
            addend: 0,
        });
    }
}

impl RelocSink {
    /// Return a new `RelocSink` instance.
    pub fn new(func_index: FuncIndex) -> Self {
        Self {
            func_index,
            func_relocs: Vec::new(),
        }
    }
}

struct TrapSink {
    pub traps: Vec<TrapInformation>,
}

impl TrapSink {
    fn new() -> Self {
        Self { traps: Vec::new() }
    }
}

impl binemit::TrapSink for TrapSink {
    fn trap(
        &mut self,
        code_offset: binemit::CodeOffset,
        source_loc: ir::SourceLoc,
        trap_code: ir::TrapCode,
    ) {
        self.traps.push(TrapInformation {
            code_offset,
            source_loc: SourceLoc::new(source_loc.bits()),
            // TODO: Translate properly environment Trapcode into cranelift IR
            trap_code: translate_ir_trapcode(trap_code),
        });
    }
}

/// Translates the Cranelift IR TrapCode into generic Trap Code
fn translate_ir_trapcode(trap: ir::TrapCode) -> TrapCode {
    match trap {
        ir::TrapCode::StackOverflow => TrapCode::StackOverflow,
        ir::TrapCode::HeapOutOfBounds => TrapCode::HeapAccessOutOfBounds,
        ir::TrapCode::TableOutOfBounds => TrapCode::TableAccessOutOfBounds,
        ir::TrapCode::OutOfBounds => TrapCode::OutOfBounds,
        ir::TrapCode::IndirectCallToNull => TrapCode::IndirectCallToNull,
        ir::TrapCode::BadSignature => TrapCode::BadSignature,
        ir::TrapCode::IntegerOverflow => TrapCode::IntegerOverflow,
        ir::TrapCode::IntegerDivisionByZero => TrapCode::IntegerDivisionByZero,
        ir::TrapCode::BadConversionToInteger => TrapCode::BadConversionToInteger,
        ir::TrapCode::UnreachableCodeReached => TrapCode::UnreachableCodeReached,
        ir::TrapCode::Interrupt => TrapCode::Interrupt,
        ir::TrapCode::User(user_code) => TrapCode::User(user_code),
    }
}

/// A compiler that compiles a WebAssembly module with Cranelift, translating the Wasm to Cranelift IR,
/// optimizing it and then translating to assembly.
pub struct CraneliftCompiler {
    isa: Box<dyn isa::TargetIsa>,
    config: CraneliftConfig,
}

impl CraneliftCompiler {
    /// Creates a new Cranelift compiler
    pub fn new(config: &CraneliftConfig) -> CraneliftCompiler {
        let isa = config.isa();
        CraneliftCompiler {
            isa,
            config: config.clone(),
        }
    }

    /// Retrieves the target ISA
    fn isa(&self) -> &dyn isa::TargetIsa {
        &*self.isa
    }

    /// Gets the WebAssembly features for this Compiler
    fn config(&self) -> &CraneliftConfig {
        &self.config
    }
}

impl Compiler for CraneliftCompiler {
    /// Gets the WebAssembly features for this Compiler
    fn features(&self) -> Features {
        self.config.features().clone()
    }

    /// Gets the target associated to the Cranelift ISA.
    fn target(&self) -> Target {
        self.config.target().clone()
    }

    /// Compile the module using Cranelift, producing a compilation result with
    /// associated relocations.
    fn compile_module(
        &self,
        module: &Module,
        module_translation: &ModuleTranslationState,
        function_body_inputs: PrimaryMap<DefinedFuncIndex, FunctionBodyData<'_>>,
        memory_plans: PrimaryMap<MemoryIndex, MemoryPlan>,
        table_plans: PrimaryMap<TableIndex, TablePlan>,
    ) -> Result<Compilation, CompileError> {
        let isa = self.isa();
        let frontend_config = isa.frontend_config();
        let signatures = module
            .signatures
            .iter()
            .map(|(_sig_index, func_type)| signature_to_cranelift_ir(func_type, &frontend_config))
            .collect::<PrimaryMap<SignatureIndex, ir::Signature>>();

        let functions = function_body_inputs
            .into_iter()
            .collect::<Vec<(DefinedFuncIndex, &FunctionBodyData<'_>)>>()
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
                let mut reloc_sink = RelocSink::new(func_index);
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
                    body: code_buf,
                    jt_offsets: func_jt_offsets,
                    unwind_info,
                    address_map,
                    relocations: reloc_sink.func_relocs,
                    traps: trap_sink.traps,
                })
            })
            .collect::<Result<Vec<_>, CompileError>>()?
            .into_iter()
            .collect::<PrimaryMap<DefinedFuncIndex, _>>();

        Ok(Compilation::new(functions))
    }

    fn compile_wasm_trampolines(
        &self,
        signatures: &[FuncType],
    ) -> Result<Vec<CompiledFunction>, CompileError> {
        signatures
            .par_iter()
            .map_init(FunctionBuilderContext::new, |mut cx, sig| {
                make_wasm_trampoline(&*self.isa, &mut cx, sig, std::mem::size_of::<u128>())
            })
            .collect::<Result<Vec<_>, CompileError>>()
    }
}

/// Transforms Cranelift JumpTable's into runtime JumpTables
pub fn transform_jump_table(
    jt_offsets: SecondaryMap<ir::JumpTable, u32>,
) -> SecondaryMap<JumpTable, u32> {
    let mut func_jt_offsets = SecondaryMap::with_capacity(jt_offsets.capacity());

    for (key, value) in jt_offsets.iter() {
        let new_key = JumpTable::new(key.index());
        func_jt_offsets[new_key] = *value;
    }
    func_jt_offsets
}
