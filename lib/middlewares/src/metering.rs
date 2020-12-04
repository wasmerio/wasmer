//! `metering` is a middleware for tracking how many operators are executed in total
//! and putting a limit on the total number of operators executed.

use std::fmt;
use std::sync::Mutex;
use wasmer::wasmparser::{
    Operator, Result as WpResult, Type as WpType, TypeOrFuncType as WpTypeOrFuncType,
};
use wasmer::{
    FunctionMiddleware, GlobalInit, GlobalType, LocalFunctionIndex, MiddlewareReaderState,
    ModuleMiddleware, Mutability, Type,
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
pub struct Metering<F: Fn(&Operator) -> u64 + Copy + Clone + Send + Sync> {
    /// Initial limit of points.
    initial_limit: u64,

    /// Function that maps each operator to a cost in "points".
    cost_function: F,

    /// The global index in the current module for remaining points.
    remaining_points_index: Mutex<Option<GlobalIndex>>,
}

/// The function-level metering middleware.
pub struct FunctionMetering<F: Fn(&Operator) -> u64 + Copy + Clone + Send + Sync> {
    /// Function that maps each operator to a cost in "points".
    cost_function: F,

    /// The global index in the current module for remaining points.
    remaining_points_index: GlobalIndex,

    /// Accumulated cost of the current basic block.
    accumulated_cost: u64,
}

impl<F: Fn(&Operator) -> u64 + Copy + Clone + Send + Sync> Metering<F> {
    /// Creates a `Metering` middleware.
    pub fn new(initial_limit: u64, cost_function: F) -> Self {
        Self {
            initial_limit,
            cost_function,
            remaining_points_index: Mutex::new(None),
        }
    }
}

impl<F: Fn(&Operator) -> u64 + Copy + Clone + Send + Sync> fmt::Debug for Metering<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Metering")
            .field("initial_limit", &self.initial_limit)
            .field("cost_function", &"<function>")
            .field("remaining_points_index", &self.remaining_points_index)
            .finish()
    }
}

impl<F: Fn(&Operator) -> u64 + Copy + Clone + Send + Sync + 'static> ModuleMiddleware
    for Metering<F>
{
    /// Generates a `FunctionMiddleware` for a given function.
    fn generate_function_middleware(&self, _: LocalFunctionIndex) -> Box<dyn FunctionMiddleware> {
        Box::new(FunctionMetering {
            cost_function: self.cost_function,
            remaining_points_index: self.remaining_points_index.lock().unwrap().expect(
                "Metering::generate_function_middleware: Remaining points index not set up.",
            ),
            accumulated_cost: 0,
        })
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

impl<F: Fn(&Operator) -> u64 + Copy + Clone + Send + Sync> fmt::Debug for FunctionMetering<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FunctionMetering")
            .field("cost_function", &"<function>")
            .field("remaining_points_index", &self.remaining_points_index)
            .finish()
    }
}

impl<F: Fn(&Operator) -> u64 + Copy + Clone + Send + Sync> FunctionMiddleware
    for FunctionMetering<F>
{
    fn feed<'a>(
        &mut self,
        operator: Operator<'a>,
        state: &mut MiddlewareReaderState<'a>,
    ) -> WpResult<()> {
        // Get the cost of the current operator, and add it to the accumulator.
        // This needs to be done before the metering logic, to prevent operators like `Call` from escaping metering in some
        // corner cases.
        self.accumulated_cost += (self.cost_function)(&operator);

        // Possible sources and targets of a branch. Finalize the cost of the previous basic block and perform necessary checks.
        match operator {
            Operator::Loop { .. } // loop headers are branch targets
            | Operator::End // block ends are branch targets
            | Operator::Else // "else" is the "end" of an if branch
            | Operator::Br { .. } // branch source
            | Operator::BrTable { .. } // branch source
            | Operator::BrIf { .. } // branch source
            | Operator::Call { .. } // function call - branch source
            | Operator::CallIndirect { .. } // function call - branch source
            | Operator::Return // end of function - branch source
            => {
                if self.accumulated_cost > 0 {
                    state.extend(&[
                        // if unsigned(globals[remaining_points_index]) < unsigned(self.accumulated_cost) { throw(); }
                        Operator::GlobalGet { global_index: self.remaining_points_index.as_u32() },
                        Operator::I64Const { value: self.accumulated_cost as i64 },
                        Operator::I64LtU,
                        Operator::If { ty: WpTypeOrFuncType::Type(WpType::EmptyBlockType) },
                        Operator::Unreachable, // FIXME: Signal the error properly.
                        Operator::End,

                        // globals[remaining_points_index] -= self.accumulated_cost;
                        Operator::GlobalGet { global_index: self.remaining_points_index.as_u32() },
                        Operator::I64Const { value: self.accumulated_cost as i64 },
                        Operator::I64Sub,
                        Operator::GlobalSet { global_index: self.remaining_points_index.as_u32() },
                    ]);

                    self.accumulated_cost = 0;
                }
            }
            _ => {}
        }
        state.push_operator(operator);

        Ok(())
    }
}
