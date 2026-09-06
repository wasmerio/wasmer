use std::path::PathBuf;
use std::ptr::NonNull;
use std::sync::Arc;
use wasmer::WASM_PAGE_SIZE;
use wasmer::sys::Features;
use wasmer::{
    MemoryError, MemoryStyle, MemoryType, Store, TableStyle, TableType,
    sys::{
        BaseTunables, CompilerConfig, ModuleMiddleware, Tunables,
        vm::{VMMemory, VMMemoryDefinition, VMTable, VMTableDefinition},
    },
};

struct DynamicMemoryTunables(BaseTunables);

impl Tunables for DynamicMemoryTunables {
    fn memory_style(&self, _memory: &MemoryType) -> MemoryStyle {
        MemoryStyle::Dynamic {
            offset_guard_size: WASM_PAGE_SIZE as u64,
        }
    }

    fn table_style(&self, table: &TableType) -> TableStyle {
        self.0.table_style(table)
    }

    fn create_host_memory(
        &self,
        ty: &MemoryType,
        style: &MemoryStyle,
    ) -> Result<VMMemory, MemoryError> {
        self.0.create_host_memory(ty, style)
    }

    unsafe fn create_vm_memory(
        &self,
        ty: &MemoryType,
        style: &MemoryStyle,
        vm_definition_location: NonNull<VMMemoryDefinition>,
    ) -> Result<VMMemory, MemoryError> {
        unsafe { self.0.create_vm_memory(ty, style, vm_definition_location) }
    }

    fn create_host_table(&self, ty: &TableType, style: &TableStyle) -> Result<VMTable, String> {
        self.0.create_host_table(ty, style)
    }

    unsafe fn create_vm_table(
        &self,
        ty: &TableType,
        style: &TableStyle,
        vm_definition_location: NonNull<VMTableDefinition>,
    ) -> Result<VMTable, String> {
        unsafe { self.0.create_vm_table(ty, style, vm_definition_location) }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Compiler {
    LLVM,
    Cranelift,
    Singlepass,
    V8,
}

#[derive(Clone)]
pub struct Config {
    pub compiler: Compiler,
    pub features: Option<Features>,
    pub middlewares: Vec<Arc<dyn ModuleMiddleware>>,
    pub canonicalize_nans: bool,
    pub allow_unaligned_memory_accesses: bool,
    pub experimental_artifact: bool,
    pub dynamic_memory: bool,
}

impl Config {
    pub fn new(compiler: Compiler) -> Self {
        Self {
            compiler,
            features: None,
            canonicalize_nans: false,
            allow_unaligned_memory_accesses: false,
            experimental_artifact: false,
            dynamic_memory: false,
            middlewares: vec![],
        }
    }

    pub fn with_experimental_artifact(mut self) -> Self {
        self.experimental_artifact = true;
        self
    }

    pub fn with_dynamic_memory(mut self) -> Self {
        self.dynamic_memory = true;
        self
    }

    pub fn set_middlewares(&mut self, middlewares: Vec<Arc<dyn ModuleMiddleware>>) {
        self.middlewares = middlewares;
    }

    pub fn set_features(&mut self, features: Features) {
        self.features = Some(features);
    }

    pub fn set_nan_canonicalization(&mut self, canonicalize_nans: bool) {
        self.canonicalize_nans = canonicalize_nans;
    }

    pub fn set_allow_unaligned_memory_accesses(&mut self, enable: bool) {
        self.allow_unaligned_memory_accesses = enable;
    }

    pub fn store(&self) -> Store {
        let engine = self.engine();
        Store::new(engine)
    }

    pub fn headless_store(&self) -> Store {
        let engine = self.engine_headless();
        Store::new(engine)
    }

    pub fn engine(&self) -> wasmer::Engine {
        match self.compiler {
            #[cfg(feature = "v8")]
            Compiler::V8 => wasmer::v8::V8::new().into(),
            _ => {
                let compiler_config = self
                    .compiler_config(self.canonicalize_nans, self.allow_unaligned_memory_accesses);
                let mut engine = wasmer::sys::EngineBuilder::new(compiler_config);
                if let Some(ref features) = self.features {
                    engine = engine.set_features(Some(features.clone()));
                }
                let mut engine = engine.engine();
                if self.dynamic_memory {
                    engine.set_tunables(DynamicMemoryTunables(BaseTunables::new()));
                }
                engine.into()
            }
        }
    }

    pub fn engine_headless(&self) -> wasmer::Engine {
        wasmer::sys::EngineBuilder::headless().engine().into()
    }

    pub fn compiler_config(
        &self,
        #[allow(unused_variables)] canonicalize_nans: bool,
        #[allow(unused_variables)] allow_unaligned_memory_accesses: bool,
    ) -> Box<dyn CompilerConfig> {
        #[allow(unused_variables)]
        let debug_dir = std::env::var("WASMER_COMPILER_DEBUG_DIR")
            .ok()
            .map(PathBuf::from);

        match &self.compiler {
            #[cfg(feature = "cranelift")]
            Compiler::Cranelift => {
                use wasmer_compiler_cranelift::CraneliftCallbacks;

                let mut compiler = wasmer_compiler_cranelift::Cranelift::new();
                compiler.experimental_artifact(self.experimental_artifact);
                compiler.canonicalize_nans(canonicalize_nans);
                compiler
                    .allow_experimental_unaligned_memory_accesses(allow_unaligned_memory_accesses);
                compiler.enable_verifier();
                if let Some(mut debug_dir) = debug_dir {
                    debug_dir.push("cranelift");
                    compiler.callbacks(Some(
                        CraneliftCallbacks::new(debug_dir)
                            .expect("cannot crate debug directory: {debug_dir}"),
                    ));
                }
                self.add_middlewares(&mut compiler);
                Box::new(compiler)
            }
            #[cfg(feature = "llvm")]
            Compiler::LLVM => {
                let mut compiler = wasmer_compiler_llvm::LLVM::new();
                compiler.experimental_artifact(self.experimental_artifact);
                compiler.canonicalize_nans(canonicalize_nans);
                compiler.enable_verifier();
                if let Some(mut debug_dir) = debug_dir {
                    use wasmer_compiler_llvm::LLVMCallbacks;
                    debug_dir.push("llvm");
                    compiler.callbacks(Some(
                        LLVMCallbacks::new(debug_dir)
                            .expect("cannot crate debug directory: {debug_dir}"),
                    ));
                }
                self.add_middlewares(&mut compiler);
                Box::new(compiler)
            }
            #[cfg(feature = "singlepass")]
            Compiler::Singlepass => {
                let mut compiler = wasmer_compiler_singlepass::Singlepass::new();
                compiler.experimental_artifact(self.experimental_artifact);
                compiler.canonicalize_nans(canonicalize_nans);
                compiler
                    .allow_experimental_unaligned_memory_accesses(allow_unaligned_memory_accesses);
                compiler.enable_verifier();
                if let Some(mut debug_dir) = debug_dir {
                    use wasmer_compiler_singlepass::SinglepassCallbacks;
                    debug_dir.push("singlepass");
                    compiler.callbacks(Some(
                        SinglepassCallbacks::new(debug_dir)
                            .expect("cannot crate debug directory: {debug_dir}"),
                    ));
                }
                self.add_middlewares(&mut compiler);
                Box::new(compiler)
            }
            #[allow(unreachable_patterns)]
            compiler => {
                panic!("The {compiler:?} Compiler is not enabled. Enable it via the features")
            }
        }
    }

    #[allow(dead_code)]
    fn add_middlewares(&self, config: &mut dyn CompilerConfig) {
        for middleware in self.middlewares.iter() {
            config.push_middleware(middleware.clone());
        }
    }
}
