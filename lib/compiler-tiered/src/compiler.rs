//! Support for compiling with Singlepass.
// Allow unused imports while developing.
#![allow(unused_imports, dead_code)]

use std::{
    convert::TryInto,
    ops::Deref,
    sync::{Arc, Mutex},
};

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
        function_body_inputs: &PrimaryMap<LocalFunctionIndex, FunctionBodyData<'_>>,
    ) -> Result<Compilation, CompileError> {
        // First attempt to compile using singlepass
        let res = self.singlepass.compile_module(
            target,
            compile_info,
            module_translation,
            function_body_inputs,
        );

        // If the compilation files then attempt to compile with cranelift instead
        // (this will occur if singlepass doesn't have compatibility)
        let res = match res {
            Ok(a) => Ok(a),
            Err(err) => {
                tracing::warn!(
                    "failed to compile with singlepass (falling back to cranelift) - {}",
                    err
                );
                self.cranelift.compile_module(
                    target,
                    compile_info,
                    module_translation,
                    function_body_inputs,
                )
            }
        };

        res
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
        // Compute the hash of the code and the compiler which is guaranteed
        // to be unique however it might not be unique enough - deeper hashing
        // is left to the caching implementation
        let hash = compute_cache(self.cranelift.deref(), function_body_inputs);

        // Check the cache (fast path)
        let caching = self.config.caching.clone();
        if let Some(compilation) = caching.try_load(hash) {
            return Some(NextArtifact::new_existing(compilation, target));
        }

        // We need to store the code we are compiling while its being compiled
        // in the background
        let function_body_inputs = {
            let mut table = PrimaryMap::<LocalFunctionIndex, _>::new();
            for (_, v) in function_body_inputs.iter() {
                table.push(v.into_owned());
            }
            table
        };

        let next = NextArtifact::new();
        {
            let next_inner = next.clone();
            let target = target.clone();
            let module = module.clone();
            let module_translation = module_translation.clone();
            let function_body_inputs = function_body_inputs.clone();
            let cranelift = self.cranelift.clone();

            // We do not immediately kick off the compilation step as the module
            // may not be able to use it (in which case a `try_upgrade` is never
            // called) and we do not want the background thread using all the CPU
            // while the `singlepass` is compiling.
            let spawn = move || {
                std::thread::spawn(move || {
                    thread_priority::set_current_thread_priority(
                        thread_priority::ThreadPriority::Min,
                    )
                    .ok();

                    let module_name = module.module.name();

                    // We wait just a short time so the program has some CPU time
                    // to actually get some stuff done before the background task
                    // kicks in
                    tracing::debug!(module_name, "cranelift module compilation yield");
                    std::thread::sleep(std::time::Duration::from_millis(200));

                    // Next we need to get a lock from the caching engine which
                    // prevents multiple background compilations of the same
                    // binary.
                    let _compile_guard_cache = caching.lock(hash);

                    // We check the cache again in case it has since been updated
                    let res = if let Some(compilation) = caching.try_load(hash) {
                        tracing::debug!(module_name, "cranelift module compilation cache hit");
                        Ok(compilation)
                    } else {
                        tracing::debug!(module_name, "cranelift module compilation started");
                        let res = cranelift.compile_module(
                            &target,
                            &module,
                            &module_translation,
                            &function_body_inputs,
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

                        // Store the value in the caching layer
                        if let Ok(compilation) = &res {
                            caching.store(hash, compilation);
                        }
                        res
                    };

                    // Return the result to anyone who's waiting for it
                    let res = CompilationResult::Ready {
                        compilation: res,
                        is_native: target.is_native(),
                    };

                    tracing::debug!(module_name, "cranelift module compilation complete");
                    next_inner.set(res);
                });
            };
            next.set(CompilationResult::Initialized {
                spawn: Box::new(spawn),
            });
        };
        Some(next)
    }
}

// Compute the hash
fn compute_cache(
    compiler: &dyn Compiler,
    function_body_inputs: &PrimaryMap<LocalFunctionIndex, FunctionBodyData<'_>>,
) -> u128 {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(compiler.name());
    for body in function_body_inputs.values() {
        hasher.update(body.data.as_ref());
    }
    let hash: [u8; 16] = hasher.finalize()[..16].try_into().unwrap();
    u128::from_le_bytes(hash)
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
