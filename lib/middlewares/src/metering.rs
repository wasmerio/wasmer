//! `metering` is a middleware for tracking how many operators are executed in total
//! and putting a limit on the total number of operators executed.

use std::fmt;
use std::sync::Mutex;
use wasmer::wasmparser::{BinaryReader, Operator, Result as WpResult};
use wasmer::{
    FunctionMiddleware, GlobalInit, GlobalType, LocalFunctionIndex, ModuleMiddleware, Mutability,
    Type,
};
use wasmer_types::GlobalIndex;
use wasmer_vm::ModuleInfo;

/// The module-level metering middleware.
///
/// # Panic
///
/// An instance of `Metering` should not be shared among different modules, since it tracks
/// module-specific information like the global index to store metering state. Attempts to use
/// a `Metering` instance from multiple modules will result in a panic.
pub struct Metering<F: Fn(&Operator) -> u64 + Send + Sync> {
    /// Initial limit of points.
    initial_limit: u64,

    /// Function that maps each operator to a cost in "points".
    cost_function: F,

    /// The global index in the current module for remaining points.
    remaining_points_index: Mutex<Option<GlobalIndex>>,
}

impl<F: Fn(&Operator) -> u64 + Send + Sync> Metering<F> {
    /// Creates a `Metering` middleware.
    pub fn new(initial_limit: u64, cost_function: F) -> Self {
        Self {
            initial_limit,
            cost_function,
            remaining_points_index: Mutex::new(None),
        }
    }
}

impl<F: Fn(&Operator) -> u64 + Send + Sync> fmt::Debug for Metering<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Metering")
            .field("initial_limit", &self.initial_limit)
            .field("cost_function", &"<function>")
            .field("remaining_points_index", &self.remaining_points_index)
            .finish()
    }
}

impl<F: Fn(&Operator) -> u64 + Send + Sync> ModuleMiddleware for Metering<F> {
    /// Generates a `FunctionMiddleware` for a given function.
    fn generate_function_middleware(&self, _: LocalFunctionIndex) -> Box<dyn FunctionMiddleware> {
        unimplemented!();
    }

    /// Transforms a `ModuleInfo` struct in-place. This is called before application on functions begins.
    fn transform_module_info(&self, module_info: &mut ModuleInfo) {
        let mut remaining_points_index = self.remaining_points_index.lock().unwrap();
        if remaining_points_index.is_some() {
            panic!("Metering::transform_module_info: Attempting to use a `Metering` middleware from multiple modules.");
        }

        // Append a global for remaining points and initialize it.
        *remaining_points_index = Some(
            module_info
                .globals
                .push(GlobalType::new(Type::I64, Mutability::Var)),
        );
        module_info
            .global_initializers
            .push(GlobalInit::I64Const(self.initial_limit as i64));
    }
}
