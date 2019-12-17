#include <stdio.h>
#include "../wasmer.h"
#include <assert.h>
#include <stdint.h>
#include <string.h>

static bool host_print_called = false;

// Host function that will be imported into the Web Assembly Instance
void host_print(const wasmer_instance_context_t *ctx, int32_t ptr, int32_t len)
{
    host_print_called = true;
    const wasmer_memory_t *memory = wasmer_instance_context_memory(ctx, 0);
    uint32_t mem_len = wasmer_memory_length(memory);
    uint8_t *mem_bytes = wasmer_memory_data(memory);
    printf("%.*s", len, mem_bytes + ptr);
}

// Use the last_error API to retrieve error messages
void print_wasmer_error()
{
    int error_len = wasmer_last_error_length();
    printf("Error len: `%d`\n", error_len);
    char *error_str = malloc(error_len);
    wasmer_last_error_message(error_str, error_len);
    printf("Error str: `%s`\n", error_str);
}

// helper function to print byte array to stdout
void print_byte_array(wasmer_byte_array *arr) {
    for (int i = 0; i < arr->bytes_len; ++i) {
        putchar(arr->bytes[i]);
    }
}

int main()
{
    // Read the Wasm file bytes.
    FILE *file = fopen("assets/emscripten_hello_world.wasm", "r");
    assert(file);
    fseek(file, 0, SEEK_END);
    long len = ftell(file);
    uint8_t *bytes = malloc(len);
    fseek(file, 0, SEEK_SET);
    fread(bytes, 1, len, file);
    fclose(file);

    wasmer_module_t *module = NULL;
    // Compile the WebAssembly module
    wasmer_result_t compile_result = wasmer_compile(&module, bytes, len);
    printf("Compile result:  %d\n", compile_result);

    if (compile_result != WASMER_OK)
    {
        print_wasmer_error();
    }

    assert(compile_result == WASMER_OK);

    // Set up data for Emscripten
    wasmer_emscripten_globals_t *emscripten_globals = wasmer_emscripten_get_globals(module);

    if (!emscripten_globals)
    {
        print_wasmer_error();
    }
    assert(emscripten_globals);

    // Create the Emscripten import object
    wasmer_import_object_t *import_object =
            wasmer_emscripten_generate_import_object(emscripten_globals);


    // Instantiatoe the module with our import_object
    wasmer_instance_t *instance = NULL;
    wasmer_result_t instantiate_result = wasmer_module_import_instantiate(&instance, module, import_object);
    printf("Instantiate result:  %d\n", instantiate_result);

    if (instantiate_result != WASMER_OK)
    {
        print_wasmer_error();
    }
    assert(instantiate_result == WASMER_OK);

    // Set up emscripten to be called
    wasmer_result_t setup_result = wasmer_emscripten_set_up(instance, emscripten_globals);
    printf("Set up result: %d\n", setup_result);

    if (setup_result != WASMER_OK)
    {
        print_wasmer_error();
    }
    assert(setup_result == WASMER_OK);


    const char *emscripten_prog_name = "emscripten_test_program";
    const char *emscripten_first_arg = "--help";
    wasmer_byte_array args[] = {
            { .bytes = (const uint8_t *) emscripten_prog_name,
              .bytes_len = strlen(emscripten_prog_name) },
            { .bytes = (const uint8_t *) emscripten_first_arg,
              .bytes_len = strlen(emscripten_first_arg) }
    };
    int emscripten_argc = sizeof(args) / sizeof(args[0]);

    wasmer_result_t main_result = wasmer_emscripten_call_main(instance, args, emscripten_argc);

    printf("Main result:  %d\n", main_result);
    assert(main_result == WASMER_OK);

    wasmer_import_object_iter_t *func_iter = wasmer_import_object_iterate_functions(import_object);

    puts("Functions in import object:");
    while ( !wasmer_import_object_iter_at_end(func_iter) ) {
            wasmer_import_t import;
            wasmer_result_t result = wasmer_import_object_iter_next(func_iter, &import);
            assert(result == WASMER_OK);

            print_byte_array(&import.module_name);
            putchar(' ');
            print_byte_array(&import.import_name);
            putchar('\n');

            assert(import.tag == WASM_FUNCTION);
            assert(import.value.func);
            wasmer_import_object_imports_destroy(&import, 1);
    }
    wasmer_import_object_iter_destroy(func_iter);

    // Use *_destroy methods to cleanup as specified in the header documentation
    wasmer_emscripten_destroy_globals(emscripten_globals);
    wasmer_instance_destroy(instance);
    wasmer_import_object_destroy(import_object);
    wasmer_module_destroy(module);

    return 0;
}

