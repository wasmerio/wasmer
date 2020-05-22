//! This module mainly outputs the `Compiler` trait that custom
//! compilers will need to implement.

use crate::error::CompileError;
use crate::function::{Compilation, FunctionBody};
use crate::lib::std::boxed::Box;
use crate::lib::std::vec::Vec;
use crate::target::Target;
use crate::FunctionBodyData;
use crate::ModuleTranslationState;
use wasm_common::entity::PrimaryMap;
use wasm_common::{
    Features, FunctionIndex, FunctionType, LocalFunctionIndex, MemoryIndex, TableIndex,
};
use wasmer_runtime::ModuleInfo;
use wasmer_runtime::{MemoryPlan, TablePlan};
use wasmparser::{validate, OperatorValidatorConfig, ValidatingParserConfig};

/// The compiler configuration options.
///
/// This options must have WebAssembly `Features` and a specific
/// `Target` to compile to.
pub trait CompilerConfig {
    /// Gets the WebAssembly features
    fn features(&self) -> &Features;

    /// Should Position Independent Code (PIC) be enabled.
    ///
    /// This is required for shared object generation (Native Engine),
    /// but will make the JIT Engine to fail, since PIC is not yet
    /// supported in the JIT linking phase.
    fn enable_pic(&mut self);

    /// Gets the target that we will use for compiling
    /// the WebAssembly module
    fn target(&self) -> &Target;

    /// Gets the custom compiler config
    fn compiler(&self) -> Box<dyn Compiler + Send>;
}

/// An implementation of a Compiler from parsed WebAssembly module to Compiled native code.
pub trait Compiler {
    /// Gets the target associated with this compiler
    fn target(&self) -> &Target;

    /// Gets the WebAssembly features for this Compiler
    fn features(&self) -> &Features;

    /// Validates a module.
    ///
    /// It returns the a succesful Result in case is valid, `CompileError` in case is not.
    fn validate_module<'data>(&self, data: &'data [u8]) -> Result<(), CompileError> {
        let features = self.features();
        let config = ValidatingParserConfig {
            operator_config: OperatorValidatorConfig {
                enable_threads: features.threads,
                enable_reference_types: features.reference_types,
                enable_bulk_memory: features.bulk_memory,
                enable_simd: features.simd,
                enable_multi_value: features.multi_value,
            },
        };
        validate(data, Some(config)).map_err(|e| CompileError::Validate(format!("{}", e)))
    }

    /// Compiles a parsed module.
    ///
    /// It returns the `Compilation` result (with a list of `CompiledFunction`)
    /// or a `CompileError`.
    fn compile_module<'data, 'module>(
        &self,
        module: &'module ModuleInfo,
        module_translation: &ModuleTranslationState,
        // The list of function bodies
        function_body_inputs: PrimaryMap<LocalFunctionIndex, FunctionBodyData<'data>>,
        // The plans for the module memories (imported and local)
        memory_plans: PrimaryMap<MemoryIndex, MemoryPlan>,
        // The plans for the module tables (imported and local)
        table_plans: PrimaryMap<TableIndex, TablePlan>,
    ) -> Result<Compilation, CompileError>;

    /// Compile the trampolines to call a function defined in
    /// a Wasm module.
    ///
    /// This allows us to call easily Wasm functions, such as:
    ///
    /// ```ignore
    /// let func = instance.exports.func("my_func");
    /// func.call(&[Value::I32(1)]);
    /// ```
    fn compile_function_call_trampolines(
        &self,
        signatures: &[FunctionType],
    ) -> Result<Vec<FunctionBody>, CompileError>;

    /// Compile the trampolines to call a dynamic function defined in
    /// a host, from a Wasm module.
    ///
    /// This allows us to create dynamic Wasm functions, such as:
    ///
    /// ```ignore
    /// fn my_func(values: Vec<Val>) -> Vec<Val> {
    /// // do something
    /// }
    ///
    /// let my_func_type = FuncType::new(vec![Type::I32], vec![Type::I32]);
    /// let imports = imports!{
    ///   "namespace" => {
    ///     "my_func" => Func::new_dynamic(my_func_type, my_func),s
    ///   }
    /// }
    /// ```
    fn compile_dynamic_function_trampolines(
        &self,
        module: &ModuleInfo,
    ) -> Result<PrimaryMap<FunctionIndex, FunctionBody>, CompileError>;
}
