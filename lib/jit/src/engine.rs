//! JIT compilation.

use crate::error::InstantiationError;
use crate::resolver::Resolver;
use crate::tunables::Tunables;
use crate::CodeMemory;
use crate::{CompiledModule, DeserializeError, SerializeError};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use wasm_common::entity::PrimaryMap;
use wasm_common::{FuncType, LocalFuncIndex, MemoryIndex, MemoryType, TableIndex, TableType};
use wasmer_compiler::{
    Compilation, CompileError, Compiler as BaseCompiler, CompilerConfig,
    FunctionBody, FunctionBodyData, ModuleTranslationState,
};
use wasmer_runtime::{
    InstanceHandle, LinearMemory, MemoryPlan, Module, SignatureRegistry, Table, TablePlan,
    VMFunctionBody, VMSharedSignatureIndex, VMTrampoline,
};

/// A WebAssembly `JIT` Engine.
pub struct JITEngine {
    inner: Arc<RefCell<JITEngineInner>>,
}

impl JITEngine {
    /// Create a new JIT Engine given config
    pub fn new<T: CompilerConfig>(config: &T) -> Self
    where
        T: ?Sized,
    {
        let compiler = config.compiler();
        let tunables = Tunables::for_target(compiler.target().triple());

        Self {
            inner: Arc::new(RefCell::new(JITEngineInner {
                compiler,
                tunables,
                trampolines: HashMap::new(),
                code_memory: CodeMemory::new(),
                signatures: SignatureRegistry::new(),
            })),
        }
    }

    fn compiler(&self) -> std::cell::Ref<'_, JITEngineInner> {
        self.inner.borrow()
    }

    fn compiler_mut(&self) -> std::cell::RefMut<'_, JITEngineInner> {
        self.inner.borrow_mut()
    }

    /// Creates a memory
    pub fn create_memory(&self, memory_type: &MemoryType) -> Result<LinearMemory, String> {
        self.compiler().create_memory(memory_type)
    }

    /// Creates a table
    pub fn create_table(&self, table_type: &TableType) -> Table {
        self.compiler().create_table(table_type)
    }

    /// Register a signature
    pub fn register_signature(&self, func_type: &FuncType) -> VMSharedSignatureIndex {
        let compiler = self.compiler();
        compiler.signatures().register(func_type)
    }

    /// Lookup a signature
    pub fn lookup_signature(&self, sig: VMSharedSignatureIndex) -> Option<FuncType> {
        let compiler = self.compiler();
        compiler.signatures().lookup(sig)
    }

    /// Retrieves a trampoline given a signature
    pub fn trampoline(&self, sig: VMSharedSignatureIndex) -> Option<VMTrampoline> {
        self.compiler().trampoline(sig)
    }

    /// Validates a WebAssembly module
    pub fn validate(&self, binary: &[u8]) -> Result<(), CompileError> {
        self.compiler().validate(binary)
    }

    /// Compile a WebAssembly binary
    pub fn compile(&self, binary: &[u8]) -> Result<CompiledModule, CompileError> {
        CompiledModule::new(&mut self.compiler_mut(), binary)
    }

    /// Instantiates a WebAssembly module
    pub fn instantiate(
        &self,
        compiled_module: &CompiledModule,
        resolver: &dyn Resolver,
    ) -> Result<InstanceHandle, InstantiationError> {
        unsafe { compiled_module.instantiate(&self.compiler(), resolver, Box::new(())) }
    }

    /// Serializes a WebAssembly module
    pub fn serialize(&self, compiled_module: &CompiledModule) -> Result<Vec<u8>, SerializeError> {
        compiled_module.serialize()
    }

    /// Deserializes a WebAssembly module
    pub fn deserialize(&self, bytes: &[u8]) -> Result<CompiledModule, DeserializeError> {
        CompiledModule::deserialize(&mut self.compiler_mut(), bytes)
    }
}

impl Clone for JITEngine {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

/// The inner contents of `JITEngine`
pub struct JITEngineInner {
    /// The compiler
    compiler: Box<dyn BaseCompiler>,
    /// The tunable values
    tunables: Tunables,
    /// Pointers to trampoline functions used to enter particular signatures
    trampolines: HashMap<VMSharedSignatureIndex, VMTrampoline>,
    /// The code memory is responsible of publishing the compiled
    /// functions to memory.
    code_memory: CodeMemory,
    /// The signature registry is used mainly to operate with trampolines
    /// performantly.
    signatures: SignatureRegistry,
}

impl JITEngineInner {
    /// Return the tunables in use by this engine.
    pub fn tunables(&self) -> &Tunables {
        &self.tunables
    }

    /// Gets the compiler associated to this JIT
    pub fn compiler(&self) -> &dyn BaseCompiler {
        &*self.compiler
    }

    /// Validate the module
    pub fn validate<'data>(&self, data: &'data [u8]) -> Result<(), CompileError> {
        self.compiler().validate_module(data)
    }

    /// Compile the given function bodies.
    pub(crate) fn compile_module<'data>(
        &mut self,
        module: &Module,
        module_translation: &ModuleTranslationState,
        function_body_inputs: PrimaryMap<LocalFuncIndex, FunctionBodyData<'data>>,
        memory_plans: PrimaryMap<MemoryIndex, MemoryPlan>,
        table_plans: PrimaryMap<TableIndex, TablePlan>,
    ) -> Result<Compilation, CompileError> {
        self.compiler.compile_module(
            module,
            module_translation,
            function_body_inputs,
            memory_plans,
            table_plans,
        )
    }

    /// Compile the given function bodies.
    pub(crate) fn compile<'data>(
        &mut self,
        module: &Module,
        functions: &PrimaryMap<LocalFuncIndex, FunctionBody>,
    ) -> Result<PrimaryMap<LocalFuncIndex, *mut [VMFunctionBody]>, CompileError> {
        // Allocate all of the compiled functions into executable memory,
        // copying over their contents.
        let allocated_functions =
            self.code_memory
                .allocate_functions(&functions)
                .map_err(|message| {
                    CompileError::Resource(format!(
                        "failed to allocate memory for functions: {}",
                        message
                    ))
                })?;

        // Trampoline generation.
        // We do it in two steps:
        // 1. Generate only the trampolines for the signatures that are unique
        // 2. Push the compiled code to memory
        let mut unique_signatures: HashMap<VMSharedSignatureIndex, FuncType> = HashMap::new();
        // for sig in module.exported_signatures() {
        for sig in module.signatures.values() {
            let index = self.signatures.register(&sig);
            if unique_signatures.contains_key(&index) {
                continue;
            }
            unique_signatures.insert(index, sig.clone());
        }

        let compiled_trampolines = self
            .compiler
            .compile_wasm_trampolines(&unique_signatures.values().cloned().collect::<Vec<_>>())?;

        for ((index, _), compiled_function) in
            unique_signatures.iter().zip(compiled_trampolines.iter())
        {
            let ptr = self
                .code_memory
                .allocate_for_function(&compiled_function.body)
                .map_err(|message| CompileError::Resource(message))?
                .as_ptr();
            let trampoline =
                unsafe { std::mem::transmute::<*const VMFunctionBody, VMTrampoline>(ptr) };
            self.trampolines.insert(*index, trampoline);
        }
        Ok(allocated_functions)
    }

    /// Make a memory plan given a memory type
    pub(crate) fn make_memory_plan(&self, memory_type: &MemoryType) -> MemoryPlan {
        self.tunables().memory_plan(memory_type.clone())
    }

    /// Make a memory plan given a memory type
    pub(crate) fn make_table_plan(&self, table_type: &TableType) -> TablePlan {
        self.tunables().table_plan(table_type.clone())
    }

    /// Create a memory given a memory type
    pub fn create_memory(&self, memory_type: &MemoryType) -> Result<LinearMemory, String> {
        let plan = self.make_memory_plan(memory_type);
        LinearMemory::new(&plan)
    }

    /// Create a memory given a memory type
    pub fn create_table(&self, table_type: &TableType) -> Table {
        let plan = self.make_table_plan(table_type);
        Table::new(&plan)
    }

    /// Make memory containing compiled code executable.
    pub(crate) fn publish_compiled_code(&mut self) {
        self.code_memory.publish();
    }

    /// Shared signature registry.
    pub fn signatures(&self) -> &SignatureRegistry {
        &self.signatures
    }

    /// Gets the trampoline pre-registered for a particular signature
    pub fn trampoline(&self, sig: VMSharedSignatureIndex) -> Option<VMTrampoline> {
        self.trampolines.get(&sig).cloned()
    }
}
