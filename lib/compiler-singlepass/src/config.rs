// Allow unused imports while developing
#![allow(unused_imports, dead_code)]

use crate::{compiler::SinglepassCompiler, machine::AssemblyComment};
use std::{
    collections::HashMap,
    fs::File,
    io::{self, Write},
    path::PathBuf,
    sync::Arc,
};
use target_lexicon::Architecture;
use wasmer_compiler::{
    Compiler, CompilerConfig, Engine, EngineBuilder, ModuleMiddleware,
    misc::{CompiledKind, function_kind_to_filename, save_assembly_to_file},
};
use wasmer_types::{
    Features,
    target::{CpuFeature, Target},
};

/// Callbacks to the different Cranelift compilation phases.
#[derive(Debug, Clone)]
pub struct SinglepassCallbacks {
    debug_dir: PathBuf,
}

impl SinglepassCallbacks {
    /// Creates a new instance of `SinglepassCallbacks` with the specified debug directory.
    pub fn new(debug_dir: PathBuf) -> Result<Self, io::Error> {
        // Create the debug dir in case it doesn't exist
        std::fs::create_dir_all(&debug_dir)?;
        Ok(Self { debug_dir })
    }

    /// Writes the object file memory buffer to a debug file.
    pub fn obj_memory_buffer(&self, kind: &CompiledKind, mem_buffer: &[u8]) {
        let mut path = self.debug_dir.clone();
        path.push(format!("{}.o", function_kind_to_filename(kind)));
        let mut file =
            File::create(path).expect("Error while creating debug file from Cranelift object");
        file.write_all(mem_buffer).unwrap();
    }

    /// Writes the assembly memory buffer to a debug file.
    pub fn asm_memory_buffer(
        &self,
        kind: &CompiledKind,
        arch: Architecture,
        mem_buffer: &[u8],
        assembly_comments: HashMap<usize, AssemblyComment>,
    ) -> Result<(), wasmer_types::CompileError> {
        let mut path = self.debug_dir.clone();
        path.push(format!("{}.s", function_kind_to_filename(kind)));
        save_assembly_to_file(arch, path, mem_buffer, assembly_comments)
    }
}

#[derive(Debug, Clone)]
pub struct Singlepass {
    pub(crate) enable_nan_canonicalization: bool,

    /// The middleware chain.
    pub(crate) middlewares: Vec<Arc<dyn ModuleMiddleware>>,

    pub(crate) callbacks: Option<SinglepassCallbacks>,
}

impl Singlepass {
    /// Creates a new configuration object with the default configuration
    /// specified.
    pub fn new() -> Self {
        Self {
            enable_nan_canonicalization: true,
            middlewares: vec![],
            callbacks: None,
        }
    }

    pub fn canonicalize_nans(&mut self, enable: bool) -> &mut Self {
        self.enable_nan_canonicalization = enable;
        self
    }

    /// Callbacks that will triggered in the different compilation
    /// phases in Singlepass.
    pub fn callbacks(&mut self, callbacks: Option<SinglepassCallbacks>) -> &mut Self {
        self.callbacks = callbacks;
        self
    }
}

impl CompilerConfig for Singlepass {
    fn enable_pic(&mut self) {
        // Do nothing, since singlepass already emits
        // PIC code.
    }

    /// Transform it into the compiler
    fn compiler(self: Box<Self>) -> Box<dyn Compiler> {
        Box::new(SinglepassCompiler::new(*self))
    }

    /// Gets the supported features for this compiler in the given target
    fn supported_features_for_target(&self, _target: &Target) -> Features {
        let mut features = Features::default();
        features.multi_value(false);
        features
    }

    /// Pushes a middleware onto the back of the middleware chain.
    fn push_middleware(&mut self, middleware: Arc<dyn ModuleMiddleware>) {
        self.middlewares.push(middleware);
    }
}

impl Default for Singlepass {
    fn default() -> Singlepass {
        Self::new()
    }
}

impl From<Singlepass> for Engine {
    fn from(config: Singlepass) -> Self {
        EngineBuilder::new(config).engine()
    }
}
