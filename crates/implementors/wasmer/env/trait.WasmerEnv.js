(function() {var implementors = {};
implementors["wasmer"] = [];
implementors["wasmer_c_api"] = [{"text":"impl WasmerEnv for LegacyEnv","synthetic":false,"types":[]}];
implementors["wasmer_emscripten"] = [{"text":"impl WasmerEnv for EmEnv","synthetic":false,"types":[]},{"text":"impl WasmerEnv for EmscriptenData","synthetic":false,"types":[]}];
implementors["wasmer_wasi"] = [{"text":"impl WasmerEnv for WasiEnv","synthetic":false,"types":[]}];
if (window.register_implementors) {window.register_implementors(implementors);} else {window.pending_implementors = implementors;}})()