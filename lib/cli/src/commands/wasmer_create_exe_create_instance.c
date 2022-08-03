
  wasm_importtype_vec_t import_types;
  wasm_module_imports(module, &import_types);

  wasm_extern_vec_t imports;
  wasm_extern_vec_new_uninitialized(&imports, import_types.size);
  wasm_importtype_vec_delete(&import_types);

#ifdef WASI
  bool get_imports_result = wasi_get_imports(store, wasi_env, module, &imports);

  if (!get_imports_result) {
    fprintf(stderr, "Error getting WASI imports!\n");
    print_wasmer_error();

    return 1;
  }
#endif

  wasm_instance_t *instance = wasm_instance_new(store, module, &imports, NULL);

  if (!instance) {
    fprintf(stderr, "Failed to create instance\n");
    print_wasmer_error();
    return -1;
  }