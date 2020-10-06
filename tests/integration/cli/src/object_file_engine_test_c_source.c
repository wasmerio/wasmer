#ifdef __cplusplus
extern "C" {
#endif

#include "wasmer_wasm.h"
#include "wasm.h"
#include "my_wasm.h"

#include <stdio.h>
#include <stdlib.h>

void wasmer_function__1(void);
void wasmer_trampoline_function_call__1(void*, void*, void*);

#ifdef __cplusplus
}
#endif

void print_wasmer_error()
{
    int error_len = wasmer_last_error_length();
    printf("Error len: `%d`\n", error_len);
    char* error_str = (char*) malloc(error_len);
    wasmer_last_error_message(error_str, error_len);
    printf("Error str: `%s`\n", error_str);
}


int main() {
        printf("Initializing...\n");
        wasm_config_t* config = wasm_config_new();
        wasm_config_set_compiler(config, CRANELIFT);
        wasm_config_set_engine(config, OBJECT_FILE);
        wasm_engine_t* engine = wasm_engine_new_with_config(config);
        wasm_store_t* store = wasm_store_new(engine);

        char* byte_ptr = (char*)&WASMER_METADATA[0];

        // We need to pass all the bytes as one big buffer so we have to do all this logic to memcpy
        // the various pieces together from the generated header file.
        //
        // We should provide a `deseralize_vectored` function to avoid requiring this extra work.

        size_t num_function_pointers
                = sizeof(function_pointers) / sizeof(void*);
        size_t num_function_trampolines
                = sizeof(function_trampolines) / sizeof(void*);
        size_t num_dynamic_function_trampoline_pointers
                = sizeof(dynamic_function_trampoline_pointers) / sizeof(void*);


        size_t buffer_size = module_bytes_len
                + sizeof(size_t) + sizeof(function_pointers)
                + sizeof(size_t) + sizeof(function_trampolines)
                + sizeof(size_t) + sizeof(dynamic_function_trampoline_pointers);

        char* memory_buffer = (char*) malloc(buffer_size);
        size_t current_offset = 0;
        printf("Buffer size: %zu\n", buffer_size);

        memcpy(memory_buffer + current_offset, byte_ptr, module_bytes_len);
        current_offset += module_bytes_len;

        memcpy(memory_buffer + current_offset, (void*)&num_function_pointers, sizeof(size_t));
        current_offset += sizeof(size_t);

        memcpy(memory_buffer + current_offset, (void*)&function_pointers[0], sizeof(function_pointers));
        current_offset += sizeof(function_pointers);

        memcpy(memory_buffer + current_offset, (void*)&num_function_trampolines, sizeof(size_t));
        current_offset += sizeof(size_t);

        memcpy(memory_buffer + current_offset, (void*)&function_trampolines[0], sizeof(function_trampolines));
        current_offset += sizeof(function_trampolines);

        memcpy(memory_buffer + current_offset, (void*)&num_dynamic_function_trampoline_pointers, sizeof(size_t));
        current_offset += sizeof(size_t);

        memcpy(memory_buffer + current_offset, (void*)&dynamic_function_trampoline_pointers[0], sizeof(dynamic_function_trampoline_pointers));
        current_offset += sizeof(dynamic_function_trampoline_pointers);

        wasm_byte_vec_t module_byte_vec = {
                .size = buffer_size,
                .data = memory_buffer,
        };

        wasm_module_t* module = wasm_module_deserialize(store, &module_byte_vec);
        if (! module) {
                printf("Failed to create module\n");
                print_wasmer_error();
                return -1;
        }
        free(memory_buffer);
        
        // We have now finished the memory buffer book keeping and we have a valid Module.

        // In this example we're passing some JavaScript source code as a command line argumnet
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
        wasi_env_set_instance(wasi_env, instance);
        
        // WASI is now set up.

        void* vmctx = wasm_instance_get_vmctx_ptr(instance);
        wasm_val_t* inout[2] = { NULL, NULL };

        fflush(stdout);
        // We're able to call our compiled functions directly through their trampolines.
        wasmer_trampoline_function_call__1(vmctx, wasmer_function__1, &inout);

        wasm_instance_delete(instance);
        wasm_module_delete(module);
        wasm_store_delete(store);
        wasm_engine_delete(engine);
        return 0;
}
