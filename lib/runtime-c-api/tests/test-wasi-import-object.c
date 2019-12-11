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
    // Create a new func to hold the parameter and signature
    // of our `host_print` host function
    wasmer_value_tag params_sig[] = {WASM_I32, WASM_I32};
    wasmer_value_tag returns_sig[] = {};
    wasmer_import_func_t *func = wasmer_import_func_new((void (*)(void *)) host_print, params_sig, 2, returns_sig, 0);

    // Create module name for our imports
    // represented in bytes for UTF-8 compatability
    const char *module_name = "env";
    wasmer_byte_array module_name_bytes;
    module_name_bytes.bytes = (const uint8_t *) module_name;
    module_name_bytes.bytes_len = strlen(module_name);

    // Define a function import
    const char *import_name = "host_print";
    wasmer_byte_array import_name_bytes;
    import_name_bytes.bytes = (const uint8_t *) import_name;
    import_name_bytes.bytes_len = strlen(import_name);
    wasmer_import_t func_import;
    func_import.module_name = module_name_bytes;
    func_import.import_name = import_name_bytes;
    func_import.tag = WASM_FUNCTION;
    func_import.value.func = func;

    // Define a memory import
    const char *import_memory_name = "memory";
    wasmer_byte_array import_memory_name_bytes;
    import_memory_name_bytes.bytes = (const uint8_t *) import_memory_name;
    import_memory_name_bytes.bytes_len = strlen(import_memory_name);
    wasmer_import_t memory_import;
    memory_import.module_name = module_name_bytes;
    memory_import.import_name = import_memory_name_bytes;
    memory_import.tag = WASM_MEMORY;
    wasmer_memory_t *memory = NULL;
    wasmer_limits_t descriptor;
    descriptor.min = 256;
    wasmer_limit_option_t max;
    max.has_some = true;
    max.some = 256;
    descriptor.max = max;
    wasmer_result_t memory_result = wasmer_memory_new(&memory, descriptor);
    if (memory_result != WASMER_OK)
    {
        print_wasmer_error();
    }
    memory_import.value.memory = memory;

    // Define a global import
    const char *import_global_name = "__memory_base";
    wasmer_byte_array import_global_name_bytes;
    import_global_name_bytes.bytes = (const uint8_t *) import_global_name;
    import_global_name_bytes.bytes_len = strlen(import_global_name);
    wasmer_import_t global_import;
    global_import.module_name = module_name_bytes;
    global_import.import_name = import_global_name_bytes;
    global_import.tag = WASM_GLOBAL;
    wasmer_value_t val;
    val.tag = WASM_I32;
    val.value.I32 = 1024;
    wasmer_global_t *global = wasmer_global_new(val, false);
    global_import.value.global = global;

    // Define a table import
    const char *import_table_name = "table";
    wasmer_byte_array import_table_name_bytes;
    import_table_name_bytes.bytes = (const uint8_t *) import_table_name;
    import_table_name_bytes.bytes_len = strlen(import_table_name);
    wasmer_import_t table_import;
    table_import.module_name = module_name_bytes;
    table_import.import_name = import_table_name_bytes;
    table_import.tag = WASM_TABLE;
    wasmer_table_t *table = NULL;
    wasmer_limits_t table_descriptor;
    table_descriptor.min = 256;
    wasmer_limit_option_t table_max;
    table_max.has_some = true;
    table_max.some = 256;
    table_descriptor.max = table_max;
    wasmer_result_t table_result = wasmer_table_new(&table, table_descriptor);
    if (table_result != WASMER_OK)
    {
        print_wasmer_error();
    }
    table_import.value.table = table;


    // Create arbitrary arguments for our program

    // Set up data for our WASI import object
    //
    // Environment variables and program arguments are processed by the WASI
    // program.  They will not have any effects unless the program includes
    // logic to process them.
    const char *wasi_prog_name = "wasi_test_program";
    const char *wasi_first_arg = "--help";
    wasmer_byte_array args[] = {
            { .bytes = (const uint8_t *) wasi_prog_name,
              .bytes_len = strlen(wasi_prog_name) },
            { .bytes = (const uint8_t *) wasi_first_arg,
              .bytes_len = strlen(wasi_first_arg) }
    };
    int wasi_argc = sizeof(args) / sizeof(args[0]);

    // Create arbitrary environment variables for our program;
    const char *wasi_color_env = "COLOR=TRUE";
    const char *wasi_app_should_log = "APP_SHOULD_LOG=FALSE";
    wasmer_byte_array envs[] = {
            { .bytes = (const uint8_t *) wasi_color_env,
              .bytes_len = strlen(wasi_color_env) },
            { .bytes = (const uint8_t *) wasi_app_should_log,
              .bytes_len = strlen(wasi_app_should_log) }
    };
    int wasi_env_len = sizeof(args) / sizeof(args[0]);

    // Open the host's current directory under a different name.
    // WARNING: this gives the WASI module limited access to your host's file system,
    // use caution when granting these permissions to untrusted Wasm modules.
    const char *wasi_map_dir_alias = "the_host_current_dir";
    const char *wasi_map_dir_host_path = ".";
    wasmer_wasi_map_dir_entry_t mapped_dirs[] = {
            { .alias =
              { .bytes = (const uint8_t *) wasi_map_dir_alias,
                .bytes_len = strlen(wasi_map_dir_alias) },
              .host_file_path =
              { .bytes = (const uint8_t *) wasi_map_dir_host_path,
                .bytes_len = strlen(wasi_map_dir_host_path) } }
    };
    int mapped_dir_len = sizeof(mapped_dirs) / sizeof(mapped_dirs[0]);

    // Read the Wasm file bytes.
    FILE *file = fopen("assets/extended_wasi.wasm", "r");
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

    // Detect the WASI version if any. This step is not mandatory, we
    // use it to test the WASI version API.
    Version wasi_version = wasmer_wasi_get_version(module);

    printf("WASI version:    %d\n", wasi_version);

    // Create the WASI import object
    wasmer_import_object_t *import_object =
        wasmer_wasi_generate_import_object_for_version(wasi_version,
                                                       args, wasi_argc,
                                                       envs, wasi_env_len,
                                                       NULL, 0,
                                                       mapped_dirs, mapped_dir_len);

    // Create our imports
    wasmer_import_t imports[] = {func_import, global_import, memory_import, table_import};
    int imports_len = sizeof(imports) / sizeof(imports[0]);
    // Add our imports to the import object
    wasmer_import_object_extend(import_object, imports, imports_len);

    // Instantiatoe the module with our import_object
    wasmer_instance_t *instance = NULL;
    wasmer_result_t instantiate_result = wasmer_module_import_instantiate(&instance, module, import_object);
    printf("Instantiate result:  %d\n", instantiate_result);

    if (instantiate_result != WASMER_OK)
    {
        print_wasmer_error();
    }
    assert(instantiate_result == WASMER_OK);

    // Call the exported "hello_wasm" function of our instance
    wasmer_value_t params[] = {};
    wasmer_value_t result_one;
    wasmer_value_t results[] = {result_one};
    // _start runs before main for WASI programs
    wasmer_result_t call_result = wasmer_instance_call(instance, "_start", params, 0, results, 1);
    printf("Call result:  %d\n", call_result);
    assert(call_result == WASMER_OK);
    assert(host_print_called);

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
    wasmer_import_func_destroy(func);
    wasmer_global_destroy(global);
    wasmer_memory_destroy(memory);
    wasmer_table_destroy(table);
    wasmer_instance_destroy(instance);
    wasmer_import_object_destroy(import_object);
    wasmer_module_destroy(module);

    return 0;
}

