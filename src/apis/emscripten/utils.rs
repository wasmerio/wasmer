use crate::webassembly::module::Module;

/// We check if a provided module is an Emscripten generated one
pub fn is_emscripten_module(module: &Module) -> bool {
    for (module, _field) in &module.info.imported_funcs {
        if module == "env" {
            return true;
        }
    }
    return false;
}

#[cfg(test)]
mod tests {
    use super::super::generate_emscripten_env;
    use super::is_emscripten_module;
    use crate::webassembly::instantiate;

    #[test]
    fn should_detect_emscripten_files() {
        let wasm_bytes = include_wast2wasm_bytes!("tests/is_emscripten_true.wast");
        let import_object = generate_emscripten_env();
        let result_object = instantiate(wasm_bytes, import_object).expect("Not compiled properly");
        assert!(is_emscripten_module(&result_object.module));
    }

    #[test]
    fn should_detect_non_emscripten_files() {
        let wasm_bytes = include_wast2wasm_bytes!("tests/is_emscripten_false.wast");
        let import_object = generate_emscripten_env();
        let result_object = instantiate(wasm_bytes, import_object).expect("Not compiled properly");
        assert!(!is_emscripten_module(&result_object.module));
    }
}
