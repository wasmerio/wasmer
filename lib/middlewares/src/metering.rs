//! `metering` is a middleware for tracking how many operators are executed in total
//! and putting a limit on the total number of operators executed.

use std::fmt;
use std::sync::Mutex;
use wasmer::wasmparser::{
    Operator, Result as WpResult, Type as WpType, TypeOrFuncType as WpTypeOrFuncType,
};
use wasmer::{
    ExportIndex, FunctionMiddleware, GlobalInit, GlobalType, Instance, LocalFunctionIndex,
    MiddlewareReaderState, ModuleMiddleware, Mutability, Type, Value,
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

    /// Get the remaining points in an Instance.
    ///
    /// Important: the instance Module must been processed with the `Metering` middleware.
    pub fn get_remaining_points(&self, instance: &Instance) -> u64 {
        instance
            .exports
            .get_global("remaining_points")
            .expect("Can't get `remaining_points` from Instance")
            .get()
            .unwrap_i64() as _
    }

    /// Set the provided remaining points in an Instance.
    ///
    /// Important: the instance Module must been processed with the `Metering` middleware.
    pub fn set_remaining_points(&self, instance: &Instance, points: u64) {
        instance
            .exports
            .get_global("remaining_points")
            .expect("Can't get `remaining_points` from Instance")
            .set(Value::I64(points as _))
            .expect("Can't set `remaining_points` in Instance");
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
        let global_index = module_info
            .globals
            .push(GlobalType::new(Type::I64, Mutability::Var));
        *remaining_points_index = Some(global_index.clone());
        module_info
            .global_initializers
            .push(GlobalInit::I64Const(self.initial_limit as i64));

        module_info.exports.insert(
            "remaining_points".to_string(),
            ExportIndex::Global(global_index),
        );
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

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;
    use wasmer::{imports, wat2wasm, CompilerConfig, Cranelift, Module, Store, JIT};

    fn cost_function(operator: &Operator) -> u64 {
        match operator {
            Operator::LocalGet { .. } | Operator::I32Const { .. } => 1,
            Operator::I32Add { .. } => 2,
            _ => 0,
        }
    }

    fn bytecode() -> Vec<u8> {
        wat2wasm(
            br#"
            (module
            (type $add_t (func (param i32) (result i32)))
            (func $add_one_f (type $add_t) (param $value i32) (result i32)
                local.get $value
                i32.const 1
                i32.add)
            (export "add_one" (func $add_one_f)))
            "#,
        )
        .unwrap()
        .into()
    }

    #[test]
    fn get_remaining_points_works() {
        let metering = Arc::new(Metering::new(10, cost_function));
        let mut compiler_config = Cranelift::default();
        compiler_config.push_middleware(metering.clone());
        let store = Store::new(&JIT::new(compiler_config).engine());
        let module = Module::new(&store, bytecode()).unwrap();

        // Instantiate
        let instance = Instance::new(&module, &imports! {}).unwrap();
        assert_eq!(metering.get_remaining_points(&instance), 10);

        // First call
        //
        // Calling add_one costs 4 points. Here are the details of how it has been computed:
        // * `local.get $value` is a `Operator::LocalGet` which costs 1 point;
        // * `i32.const` is a `Operator::I32Const` which costs 1 point;
        // * `i32.add` is a `Operator::I32Add` which costs 2 points.
        let add_one = instance
            .exports
            .get_function("add_one")
            .unwrap()
            .native::<i32, i32>()
            .unwrap();
        add_one.call(1).unwrap();
        assert_eq!(metering.get_remaining_points(&instance), 6);

        // Second call
        add_one.call(1).unwrap();
        assert_eq!(metering.get_remaining_points(&instance), 2);

        // Third call fails due to limit
        assert!(add_one.call(1).is_err());
        // TODO: what do we expect now? 0 or 2? See https://github.com/wasmerio/wasmer/issues/1931
        // assert_eq!(metering.get_remaining_points(&instance), 2);
        // assert_eq!(metering.get_remaining_points(&instance), 0);
    }

    #[test]
    fn set_remaining_points_works() {
        let metering = Arc::new(Metering::new(10, cost_function));
        let mut compiler_config = Cranelift::default();
        compiler_config.push_middleware(metering.clone());
        let store = Store::new(&JIT::new(compiler_config).engine());
        let module = Module::new(&store, bytecode()).unwrap();

        // Instantiate
        let instance = Instance::new(&module, &imports! {}).unwrap();
        assert_eq!(metering.get_remaining_points(&instance), 10);
        let add_one = instance
            .exports
            .get_function("add_one")
            .unwrap()
            .native::<i32, i32>()
            .unwrap();

        // Increase a bit to have enough for 3 calls
        metering.set_remaining_points(&instance, 12);

        // Ensure we can use the new points now
        add_one.call(1).unwrap();
        assert_eq!(metering.get_remaining_points(&instance), 8);
        add_one.call(1).unwrap();
        assert_eq!(metering.get_remaining_points(&instance), 4);
        add_one.call(1).unwrap();
        assert_eq!(metering.get_remaining_points(&instance), 0);
    }
}
