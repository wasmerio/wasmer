use crate::instance::Instance;
use crate::WasmerEnv;
use core::any::TypeId;
use js_sys::Function;
use js_sys::WebAssembly::Memory;
use std::any::Any;
use std::cell::RefCell;
use std::fmt;
use std::sync::Arc;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;

pub type VMMemory = Memory;
#[derive(Clone)]
pub struct VMFunction {
    pub(crate) function: Function,
    pub(crate) environment: Option<Arc<RefCell<Box<dyn WasmerEnv>>>>,
}

impl VMFunction {
    pub(crate) fn new(function: Function, environment: Option<Box<dyn WasmerEnv>>) -> Self {
        Self {
            function,
            environment: environment.map(|env| Arc::new(RefCell::new(env))),
        }
    }
    pub(crate) fn init_envs(&self, instance: &Instance) {
        if let Some(env) = &self.environment {
            let mut borrowed_env = env.borrow_mut();
            borrowed_env.init_with_instance(instance);
        }
    }
}

impl PartialEq for VMFunction {
    fn eq(&self, other: &Self) -> bool {
        self.function == other.function
    }
}

impl fmt::Debug for VMFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VMFunction")
            .field("function", &self.function)
            .finish()
    }
}

/// The value of an export passed from one instance to another.
#[derive(Debug, Clone)]
pub enum Export {
    /// A function export value.
    Function(VMFunction),

    // /// A table export value.
    // Table(VMTable),
    /// A memory export value.
    Memory(VMMemory),
    // /// A global export value.
    // Global(VMGlobal),
}

impl Export {
    pub fn as_jsvalue(&self) -> &JsValue {
        match self {
            Export::Memory(js_wasm_memory) => js_wasm_memory.as_ref(),
            Export::Function(js_func) => js_func.function.as_ref(),
            _ => unimplemented!(),
        }
    }
}

impl From<JsValue> for Export {
    fn from(val: JsValue) -> Export {
        if val.is_instance_of::<Memory>() {
            return Export::Memory(val.unchecked_into::<Memory>());
        }
        // Leave this last
        else if val.is_instance_of::<Function>() {
            return Export::Function(VMFunction::new(val.unchecked_into::<Function>(), None));
        }
        unimplemented!();
    }
}
