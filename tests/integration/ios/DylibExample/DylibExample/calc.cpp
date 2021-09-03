//
//  WASM.cpp
//  DylibExample
//
//  Created by Nathan Horrigan on 17/08/2021.
//

#include "calc.h"

#include <any>
#include <cstdio>
#include <cstring>
#include <fstream>
#include <iostream>
#include <sstream>
#include <vector>
#include <filesystem>
#include <CoreFoundation/CFBundle.h>

std::string get_resources_dir()
{

  CFURLRef resourceURL = CFBundleCopyResourcesDirectoryURL(CFBundleGetMainBundle());
  char resourcePath[PATH_MAX];
  if (CFURLGetFileSystemRepresentation(resourceURL, true,
                                       (UInt8 *)resourcePath,
                                       PATH_MAX))
  {
    if (resourceURL != NULL)
    {
      CFRelease(resourceURL);
    }

    return resourcePath;
  }

  return "";
}

inline std::vector<uint8_t> read_vector_from_disk(std::string file_path)
{
  std::ifstream instream(file_path, std::ios::in | std::ios::binary);
  std::vector<uint8_t> data((std::istreambuf_iterator<char>(instream)), std::istreambuf_iterator<char>());
  return data;
}

int calculate_sum(int a, int b)
{
  printf("Creating the store...\n");
  wasm_engine_t *engine = wasm_engine_new();
  wasm_store_t *store = wasm_store_new(engine);

  printf("Loading .dylib file...\n");
  std::string wasm_path = get_resources_dir() + "/sum.dylib";
  std::vector<uint8_t> dylib = read_vector_from_disk(wasm_path.c_str());
  uint8_t *wasm_bytes = dylib.data();

  wasm_byte_vec_t imported_bytes;
  imported_bytes.size = dylib.size();
  imported_bytes.data = (wasm_byte_t *)wasm_bytes;

  printf("Compiling module...\n");
  wasm_module_t *module;
  module = wasm_module_deserialize(store, &imported_bytes);

  if (!module)
  {
    printf("> Error compiling module!\n");

    return 1;
  }

  printf("Creating imports...\n");
  wasm_extern_vec_t import_object = WASM_EMPTY_VEC;

  printf("Instantiating module...\n");
  wasm_instance_t *instance = wasm_instance_new(store, module, &import_object, NULL);

  if (!instance)
  {
    printf("> Error instantiating module!\n");

    return 1;
  }

  printf("Retrieving exports...\n");
  wasm_extern_vec_t exports;
  wasm_instance_exports(instance, &exports);

  if (exports.size == 0)
  {
    printf("> Error accessing exports!\n");

    return 1;
  }

  printf("Retrieving the `sum` function...\n");
  wasm_func_t *sum_func = wasm_extern_as_func(exports.data[0]);

  if (sum_func == NULL)
  {
    printf("> Failed to get the `sum` function!\n");

    return 1;
  }

  printf("Calling `sum` function...\n");
  wasm_val_t args_val[2] = {WASM_I32_VAL(a), WASM_I32_VAL(b)};
  wasm_val_t results_val[1] = {WASM_INIT_VAL};
  wasm_val_vec_t args = WASM_ARRAY_VEC(args_val);
  wasm_val_vec_t results = WASM_ARRAY_VEC(results_val);

  if (wasm_func_call(sum_func, &args, &results))
  {
    printf("> Error calling the `sum` function!\n");

    return 1;
  }

  printf("Results of `sum`: %d\n", results_val[0].of.i32);

  return results_val[0].of.i32;
}
