//! This file provides suggestions for the user, to help them on the
//! usage of WebAssembly
use distance::damerau_levenshtein;
use wasmer::Module;

/// Suggest function exports for the module
pub fn suggest_function_exports(module: &Module, query: &str) -> Vec<String> {
    let mut function_names = module
        .exports()
        .functions()
        .map(|extern_fn| {
            let name = extern_fn.name();
            name.to_string()
        })
        .collect::<Vec<_>>();
    function_names.sort_by_key(|name| damerau_levenshtein(name, query));
    function_names
}
