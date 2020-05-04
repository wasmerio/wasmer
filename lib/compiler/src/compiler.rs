//! This module mainly outputs the `Compiler` trait that custom
//! compilers will need to implement.

use crate::error::CompileError;
use crate::function::{Compilation, FunctionBody};
use crate::std::boxed::Box;
use crate::std::vec::Vec;
use crate::target::Target;
use crate::FunctionBodyData;
use crate::ModuleTranslationState;
use wasm_common::entity::PrimaryMap;
use wasm_common::{Features, FunctionType, LocalFuncIndex, MemoryIndex, TableIndex};
use wasmer_runtime::Module;
use wasmer_runtime::{MemoryPlan, TablePlan};
use wasmparser::{validate, OperatorValidatorConfig, ValidatingParserConfig};

/// The compiler configuration options.
///
/// This options must have WebAssembly `Features` and a specific
/// `Target` to compile to.
pub trait CompilerConfig {
    /// Gets the WebAssembly features
    fn features(&self) -> &Features;

    /// Gets the target that we will use for compiling
    /// the WebAssembly module
    fn target(&self) -> &Target;

    /// Gets the custom compiler config
    fn compiler(&self) -> Box<dyn Compiler>;
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
        module: &'module Module,
        module_translation: &ModuleTranslationState,
        // The list of function bodies
        function_body_inputs: PrimaryMap<LocalFuncIndex, FunctionBodyData<'data>>,
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
    fn compile_wasm_trampolines(
        &self,
        signatures: &[FunctionType],
    ) -> Result<Vec<FunctionBody>, CompileError>;
}
