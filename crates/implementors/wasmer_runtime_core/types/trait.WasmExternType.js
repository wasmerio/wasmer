(function() {var implementors = {};
implementors["wasmer_emscripten"] = [{"text":"impl&lt;T:&nbsp;<a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Copy.html\" title=\"trait core::marker::Copy\">Copy</a>, Ty&gt; <a class=\"trait\" href=\"wasmer_runtime_core/types/trait.WasmExternType.html\" title=\"trait wasmer_runtime_core::types::WasmExternType\">WasmExternType</a> for <a class=\"struct\" href=\"wasmer_emscripten/ptr/struct.WasmPtr.html\" title=\"struct wasmer_emscripten::ptr::WasmPtr\">WasmPtr</a>&lt;T, Ty&gt;","synthetic":false,"types":["wasmer_emscripten::ptr::WasmPtr"]},{"text":"impl <a class=\"trait\" href=\"wasmer_runtime_core/types/trait.WasmExternType.html\" title=\"trait wasmer_runtime_core::types::WasmExternType\">WasmExternType</a> for <a class=\"struct\" href=\"wasmer_emscripten/varargs/struct.VarArgs.html\" title=\"struct wasmer_emscripten::varargs::VarArgs\">VarArgs</a>","synthetic":false,"types":["wasmer_emscripten::varargs::VarArgs"]}];
implementors["wasmer_runtime_core"] = [];
implementors["wasmer_wasi"] = [{"text":"impl&lt;T:&nbsp;<a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Copy.html\" title=\"trait core::marker::Copy\">Copy</a>, Ty&gt; <a class=\"trait\" href=\"wasmer_runtime_core/types/trait.WasmExternType.html\" title=\"trait wasmer_runtime_core::types::WasmExternType\">WasmExternType</a> for <a class=\"struct\" href=\"wasmer_wasi/ptr/struct.WasmPtr.html\" title=\"struct wasmer_wasi::ptr::WasmPtr\">WasmPtr</a>&lt;T, Ty&gt;","synthetic":false,"types":["wasmer_wasi::ptr::WasmPtr"]}];

            if (window.register_implementors) {
                window.register_implementors(implementors);
            } else {
                window.pending_implementors = implementors;
            }
        })()