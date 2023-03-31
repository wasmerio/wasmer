//! Support for compiling with Singlepass.
// Allow unused imports while developing.
#![allow(unused_imports, dead_code)]

use std::sync::Arc;

use crate::Tiered;
use enumset::EnumSet;
use wasmer_compiler::{
    ArtifactBuild, CompilationResult, Compiler, CompilerConfig, FunctionBodyData, ModuleMiddleware,
    ModuleTranslationState, NextArtifact,
};
use wasmer_compiler_cranelift::CraneliftCompiler;
use wasmer_compiler_singlepass::SinglepassCompiler;
use wasmer_types::{
    entity::PrimaryMap, Compilation, CompileError, CompileModuleInfo, CpuFeature,
    LocalFunctionIndex, OwnedDataInitializer, Target,
};

/// A compiler that compiles a WebAssembly module with Tiered compilation.
pub struct TieredCompiler {
    config: Tiered,
    singlepass: SinglepassCompiler,
    cranelift: Arc<CraneliftCompiler>,
}

impl TieredCompiler {
    /// Creates a new Tiered compiler
    pub fn new(config: Tiered) -> Self {
        let singlepass = config.clone().singlepass;
        let cranelift = config.clone().cranelift;
        Self {
            config,
            singlepass: SinglepassCompiler::new(*singlepass),
            cranelift: Arc::new(CraneliftCompiler::new(*cranelift)),
        }
    }

    /// Gets the config for this Compiler
    fn config(&self) -> &Tiered {
        &self.config
    }
}

impl Compiler for TieredCompiler {
    fn name(&self) -> &str {
        "tiered"
    }

    fn get_middlewares(&self) -> &[Arc<dyn ModuleMiddleware>] {
        &self.config.middlewares
    }

    fn compile_module(
        &self,
        target: &Target,
        compile_info: &CompileModuleInfo,
        module_translation: &ModuleTranslationState,
        function_body_inputs: PrimaryMap<LocalFunctionIndex, FunctionBodyData<'_>>,
    ) -> Result<Compilation, CompileError> {
        self.singlepass.compile_module(
            target,
            compile_info,
            module_translation,
            function_body_inputs,
        )
    }

    fn get_cpu_features_used(&self, cpu_features: &EnumSet<CpuFeature>) -> EnumSet<CpuFeature> {
        self.singlepass
            .get_cpu_features_used(cpu_features)
            .intersection(self.cranelift.get_cpu_features_used(cpu_features))
    }

    fn get_next_artifact(
        &self,
        target: &Target,
        module: &CompileModuleInfo,
        module_translation: &ModuleTranslationState,
        function_body_inputs: &PrimaryMap<LocalFunctionIndex, FunctionBodyData<'_>>,
        data_initializers: Box<[OwnedDataInitializer]>,
        cpu_features: EnumSet<CpuFeature>,
    ) -> Option<NextArtifact> {
        let function_body_inputs = {
            let mut table = PrimaryMap::<LocalFunctionIndex, _>::new();
            for (_, v) in function_body_inputs.iter() {
                table.push(v.into_owned());
            }
            table
        };

        let next = NextArtifact::new();
        {
            let next = next.clone();
            let target = target.clone();
            let module = module.clone();
            let module_translation = module_translation.clone();
            let function_body_inputs = function_body_inputs.clone();
            let cranelift = self.cranelift.clone();
            std::thread::spawn(move || {
                thread_priority::set_current_thread_priority(thread_priority::ThreadPriority::Min)
                    .ok();

                let function_body_inputs = {
                    let mut table = PrimaryMap::new();
                    for (_, v) in function_body_inputs.iter() {
                        table.push(v.into_ref());
                    }
                    table
                };

                let res = cranelift.compile_module(
                    &target,
                    &module,
                    &module_translation,
                    function_body_inputs,
                );
                let res = res.map(|compilation| {
                    ArtifactBuild::convert_to_serializable(
                        compilation,
                        &target,
                        cpu_features,
                        module,
                        data_initializers,
                    )
                });
                let res = CompilationResult::Ready {
                    compilation: res,
                    target,
                };

                next.set(res);
            });
        };
        Some(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    use wasmer_compiler::Features;
    use wasmer_types::{
        CpuFeature, MemoryIndex, MemoryStyle, ModuleInfo, TableIndex, TableStyle, Triple,
    };

    fn dummy_compilation_ingredients<'a>() -> (
        CompileModuleInfo,
        ModuleTranslationState,
        PrimaryMap<LocalFunctionIndex, FunctionBodyData<'a>>,
    ) {
        let compile_info = CompileModuleInfo {
            features: Features::new(),
            module: Arc::new(ModuleInfo::new()),
            memory_styles: PrimaryMap::<MemoryIndex, MemoryStyle>::new(),
            table_styles: PrimaryMap::<TableIndex, TableStyle>::new(),
        };
        let module_translation = ModuleTranslationState::new();
        let function_body_inputs = PrimaryMap::<LocalFunctionIndex, FunctionBodyData<'_>>::new();
        (compile_info, module_translation, function_body_inputs)
    }
}
