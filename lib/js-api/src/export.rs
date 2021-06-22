use js_sys::Function;
use js_sys::WebAssembly::Memory;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;

pub type VMMemory = Memory;
pub type VMFunction = Function;

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
            Export::Function(js_func) => js_func.as_ref(),
            _ => unimplemented!(),
        }
    }
}

impl From<JsValue> for Export {
    fn from(val: JsValue) -> Export {
        if val.is_instance_of::<VMMemory>() {
            return Export::Memory(val.unchecked_into::<VMMemory>());
        }
        // Leave this last
        else if val.is_instance_of::<VMFunction>() {
            return Export::Function(val.unchecked_into::<VMFunction>());
        }
        unimplemented!();
    }
}
