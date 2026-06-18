//! Define `ArtifactBuild` to allow compiling and instantiating to be
//! done as separate steps.

#[cfg(feature = "compiler")]
use crate::translator::analyze_readonly_funcref_table;
#[cfg(feature = "compiler")]
use crate::{EngineInner, ModuleEnvironment, ModuleMiddlewareChain};
use crate::{serialize::SerializableModule, types::module::CompileModuleInfo};
use tempfile::NamedTempFile;
#[cfg(feature = "compiler")]
use wasmer_types::{CompilationProgressCallback, target::Target};

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use wasmer_types::entity::PrimaryMap;

// Not every compiler backend uses these.
#[allow(unused)]
use wasmer_types::*;

pub(crate) enum ModuleFile {
    TempFile(NamedTempFile),
    OwnedFile(PathBuf),
}

impl ModuleFile {
    pub(crate) fn path(&self) -> &Path {
        match self {
            Self::OwnedFile(path) => path.as_path(),
            Self::TempFile(tempfile) => tempfile.path(),
        }
    }
}

/// A compiled wasm module, ready to be instantiated.
#[cfg_attr(feature = "artifact-size", derive(loupe::MemoryUsage))]
pub struct ArtifactBuild {
    pub(crate) serializable: SerializableModule,
    pub(crate) module_file: ModuleFile,
}

impl ArtifactBuild {
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

        module.hash = Some(ModuleHash::new(data));
        let compile_info = CompileModuleInfo {
            module: Arc::new(module),
            features,
            memory_styles,
            table_styles,
        };
        let cpu_features = compiler.get_cpu_features_used(target.cpu_features());
        let data_initializers = translation
            .data_initializers
            .iter()
            .map(OwnedDataInitializer::new)
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let serializable = SerializableModule {
            compile_info,
            data_initializers,
            cpu_features: cpu_features.as_u64(),
        };

        // Compile the Module
        let module_file = compiler.compile_module(
            target,
            &serializable.compile_info,
            rkyv::to_bytes::<rkyv::rancor::Error>(&serializable)
                // TODO
                .map(|bytes| bytes.into_vec())
                .unwrap(),
            // SAFETY: Calling `unwrap` is correct since
            // `environ.translate()` above will write some data into
            // `module_translation_state`.
            translation.module_translation_state.as_ref().unwrap(),
            translation.function_body_inputs,
            progress_callback,
        )?;

        Ok(Self {
            serializable,
            module_file: ModuleFile::TempFile(module_file),
        })
    }
}
