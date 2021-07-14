# Wasmer Engine Staticlib

This is an [engine](https://crates.io/crates/wasmer-engine) for the
[wasmer](https://crates.io/crates/wasmer) WebAssembly VM.

This engine is used to produce a native static object library that can
be linked against providing a sandboxed WebAssembly runtime
environment for the compiled module with no need for runtime
compilation.

## Example of use

First we compile our WebAssembly file with Wasmer

```sh
wasmer compile path/to/wasm/file.wasm --llvm --staticlib -o my_wasm.o --header my_wasm.h
```

You will then see output like:

```
Engine: staticlib
Compiler: llvm
Target: x86_64-apple-darwin
✔ File compiled successfully to `my_wasm.o`.
✔ Header file generated successfully at `my_wasm.h`.
```

Now let's create a program to link with this static object file.

```c
#include "wasmer.h"
#include "my_wasm.h"

#include <stdio.h>
#include <stdlib.h>

#define own

static void print_wasmer_error()
{
    int error_len = wasmer_last_error_length();
    printf("Error len: `%d`\n", error_len);
    char* error_str = (char*) malloc(error_len);
    wasmer_last_error_message(error_str, error_len);
    printf("Error str: `%s`\n", error_str);
    free(error_str);
}

int main() {
        printf("Initializing...\n");
        wasm_config_t* config = wasm_config_new();
        wasm_config_set_engine(config, STATICLIB);
        wasm_engine_t* engine = wasm_engine_new_with_config(config);
        wasm_store_t* store = wasm_store_new(engine);

        wasm_module_t* module = wasmer_staticlib_engine_new(store, "qjs.wasm");
        if (!module) {
                printf("Failed to create module\n");
                print_wasmer_error();
                return -1;
        }
        
        // We have now finished the memory buffer book keeping and we have a valid Module.

        // In this example we're passing some JavaScript source code as a command line argument
        // to a WASI module that can evaluate JavaScript.
        wasi_config_t* wasi_config = wasi_config_new("constant_value_here");
        const char* js_string = "function greet(name) { return JSON.stringify('Hello, ' + name); }; print(greet('World'));";
        wasi_config_arg(wasi_config, "--eval");
        wasi_config_arg(wasi_config, js_string);
        wasi_env_t* wasi_env = wasi_env_new(wasi_config);
        if (!wasi_env) {
                printf("> Error building WASI env!\n");
                print_wasmer_error();
                return 1;
        }

        wasm_importtype_vec_t import_types;
        wasm_module_imports(module, &import_types);
        int num_imports = import_types.size;
        wasm_extern_t** imports = (wasm_extern_t**) malloc(num_imports * sizeof(wasm_extern_t*));
        wasm_importtype_vec_delete(&import_types);
        
        bool get_imports_result = wasi_get_imports(store, module, wasi_env, imports);
        wasi_env_delete(wasi_env);
        if (!get_imports_result) {
                printf("> Error getting WASI imports!\n");
                print_wasmer_error();
                return 1;
        }

        wasm_instance_t* instance = wasm_instance_new(store, module, (const wasm_extern_t* const*) imports, NULL);
        if (! instance) {
                printf("Failed to create instance\n");
                print_wasmer_error();
                return -1;
        }
        
        // WASI is now set up.
        own wasm_func_t* start_function = wasi_get_start_function(instance);
        if (!start_function) {
                fprintf(stderr, "`_start` function not found\n");
                print_wasmer_error();
                return -1;
        }

        fflush(stdout);
        own wasm_trap_t* trap = wasm_func_call(start_function, NULL, NULL);
        if (trap) {
                fprintf(stderr, "Trap is not NULL: TODO:\n");
                return -1;
        }

        wasm_instance_delete(instance);
        wasm_module_delete(module);
        wasm_store_delete(store);
        wasm_engine_delete(engine);
        return 0;
}
```

We save that source code into `test.c` and run:

```sh
clang -O2 -c test.c -o test.o
```

Now we just need to link everything together:

```sh
clang -O2 test.o my_wasm.o libwasmer.a
```

We link the static object file we created with our C code, the object
file we generated with Wasmer, and `libwasmer` together and produce an
executable that can call into our compiled WebAssembly!
