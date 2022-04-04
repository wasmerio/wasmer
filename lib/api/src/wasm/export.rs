pub use wasmer_fakevm::{VMFunction, VMGlobal, VMMemory, VMTable};

/// The value of an export passed from one instance to another.
#[derive(Debug, Clone)]
pub enum Export {
    /// A function export value.
    Function(VMFunction),

    /// A table export value.
    Table(VMTable),

    /// A memory export value.
    Memory(VMMemory),

    /// A global export value.
    Global(VMGlobal),
}
/*
impl Export {
    /// Return the export as a `JSValue`.
    pub fn as_jsvalue(&self) -> &JsValue {
        match self {
            Export::Memory(_wasm_memory) => panic!("Not impleented!"),
            Export::Function(_func) => panic!("Not impleented!"),
            Export::Table(_wasm_table) => panic!("Not impleented!"),
            Export::Global(_wasm_global) => panic!("Not impleented!"),
        }
    }
}
*/
