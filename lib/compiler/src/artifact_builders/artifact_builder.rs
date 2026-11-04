//! Define `ArtifactBuild` to allow compiling and instantiating to be
//! done as separate steps.

#[cfg(feature = "compiler")]
use crate::translator::analyze_readonly_funcref_table;
use crate::{
    ArtifactCreate, Features,
    types::{function::Compilation, module::CompileModuleInfo},
};
#[cfg(feature = "compiler")]
use crate::{EngineInner, ModuleEnvironment, ModuleMiddlewareChain};
#[cfg(feature = "compiler")]
use wasmer_types::{CompilationProgressCallback, target::Target};

use enumset::EnumSet;
use std::sync::Arc;
use wasmer_types::{entity::PrimaryMap, target::CpuFeature};

// Not every compiler backend uses these.
#[allow(unused)]
use wasmer_types::*;

/// A compiled wasm module, ready to be instantiated.
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
pub struct ArtifactBuild {
    pub(crate) compilation: Compilation,
    pub(crate) compile_info: CompileModuleInfo,
}

impl ArtifactBuild {
    /// Check if the provided bytes look like a serialized `ArtifactBuild`.
    pub fn is_deserializable(bytes: &[u8]) -> bool {
        bytes.starts_with(&object::elf::ELFMAG)
            || bytes.starts_with(&object::macho::MH_MAGIC_64.to_le_bytes())
    }

    /// Compile a data buffer into a `ArtifactBuild`, which may then be instantiated.
    #[cfg(feature = "compiler")]
    pub fn new(
        inner_engine: &mut EngineInner,
        data: &[u8],
        target: &Target,
        memory_styles: PrimaryMap<MemoryIndex, MemoryStyle>,
        table_styles: PrimaryMap<TableIndex, TableStyle>,
        progress_callback: Option<&CompilationProgressCallback>,
    ) -> Result<Self, CompileError> {
        let environ = ModuleEnvironment::new();
        let features = inner_engine.features().clone();
        let translation = environ.translate(data).map_err(CompileError::Wasm)?;
        let compiler = inner_engine.compiler()?;

        // We try to apply the middleware first
        let mut module = translation.module;
        let middlewares = compiler.get_middlewares();
        middlewares
            .apply_on_module_info(&mut module)
            .map_err(|err| CompileError::MiddlewareError(err.to_string()))?;
        #[cfg(feature = "translator")]
        if compiler.enable_readonly_funcref_table()
            && let Some(table_index) =
                analyze_readonly_funcref_table(&module, &translation.function_body_inputs)?
        {
            module.tables[table_index].readonly = true;
        }

        module.m0_pass_param_enabled = compiler.m0_pass_param_enabled(&memory_styles);
        module.hash = Some(ModuleHash::new(data));
        let cpu_features = compiler.get_cpu_features_used(target.cpu_features());
        let mut compile_info = CompileModuleInfo {
            module: Arc::new(module),
            features,
            memory_styles,
            table_styles,
            function_max_stack_usage: PrimaryMap::new(),
            data_initializers: translation
                .data_initializers
                .iter()
                .map(OwnedDataInitializer::new)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            cpu_features: cpu_features.as_u64(),
        };
        let compilation = compiler.compile_module(
            target,
            &mut compile_info,
            // SAFETY: Calling `unwrap` is correct since
            // `environ.translate()` above will write some data into
            // `module_translation_state`.
            translation.module_translation_state.as_ref().unwrap(),
            translation.function_body_inputs,
            progress_callback,
        )?;

        Ok(Self {
            compilation,
            compile_info,
        })
    }

    /// The maximum stack allocation directly connected to the function itself
    /// if tracked (does not include any potential function calls).
    /// Available only for the Singlepass compiler
    pub fn get_function_max_stack_usage(
        &self,
    ) -> Option<&PrimaryMap<LocalFunctionIndex, Option<usize>>> {
        Some(&self.compile_info.function_max_stack_usage)
    }
}

impl<'a> ArtifactCreate<'a> for ArtifactBuild {
    type OwnedDataInitializer = &'a OwnedDataInitializer;
    type OwnedDataInitializerIterator = core::slice::Iter<'a, OwnedDataInitializer>;

    fn create_module_info(&self) -> Arc<ModuleInfo> {
        self.compile_info.module.clone()
    }

    fn set_module_info_name(&mut self, name: String) -> bool {
        Arc::get_mut(&mut self.compile_info.module).is_some_and(|module_info| {
            module_info.name = Some(name.to_string());
            true
        })
    }

    fn module_info(&self) -> &ModuleInfo {
        &self.compile_info.module
    }

    fn features(&self) -> &Features {
        &self.compile_info.features
    }

    fn cpu_features(&self) -> EnumSet<CpuFeature> {
        EnumSet::from_u64(self.compile_info.cpu_features)
    }

    fn data_initializers(&'a self) -> Self::OwnedDataInitializerIterator {
        self.compile_info.data_initializers.iter()
    }

    fn memory_styles(&self) -> &PrimaryMap<MemoryIndex, MemoryStyle> {
        &self.compile_info.memory_styles
    }

    fn table_styles(&self) -> &PrimaryMap<TableIndex, TableStyle> {
        &self.compile_info.table_styles
    }

    fn serialize(&self) -> Result<Vec<u8>, SerializeError> {
        Ok(self.compilation.clone())
    }
}
