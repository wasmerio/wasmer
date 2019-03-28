
use wasmer_runtime_core::{
    import::ImportObject,
    imports,
    func,
};

pub fn generate_import_object() -> ImportObject {
    imports! {
        // This generates the wasi state.
        || {
            // returns (pointer to state, function that can destruct the state).
        },
        "wasi_unstable" => {
            
        },
    }
}