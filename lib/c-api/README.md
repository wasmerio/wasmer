# `wasmer-c-api` [![Build Status](https://github.com/wasmerio/wasmer/workflows/build/badge.svg?style=flat-square)](https://github.com/wasmerio/wasmer/actions?query=workflow%3Abuild) [![Join Wasmer Slack](https://img.shields.io/static/v1?label=Slack&message=join%20chat&color=brighgreen&style=flat-square)](https://slack.wasmer.io) [![MIT License](https://img.shields.io/github/license/wasmerio/wasmer.svg?style=flat-square)](https://github.com/wasmerio/wasmer/blob/master/LICENSE)

This crate exposes a C and a C++ API for the Wasmer runtime. It also fully supports the [wasm-c-api common API](https://github.com/WebAssembly/wasm-c-api).

##  Usage

Once you [install Wasmer in your system](https://github.com/wasmerio/wasmer-install), the *shared object files* and the *headers* are available **inside the Wasmer installed path**.

```
$WASMER_DIR/
  lib/
    libwasmer.{so,dylib,dll}
  include/
    wasm.h
    wasmer.h
    wasmer.hh
    wasmer.h
```

Wasmer binary also ships with [`wasmer-config`](#wasmer-config)
an utility tool that outputs config information needed to compile programs which use Wasmer.

[The full C API documentation can be found here](https://wasmerio.github.io/wasmer/crates/doc/wasmer_c_api/).

Here is a simple example to use the C API:

```c
#include <stdio.h>
#include "wasmer.h"

int main(int argc, const char* argv[]) {
    const char *wat_string =
        "(module\n"
        "  (type $sum_t (func (param i32 i32) (result i32)))\n"
        "  (func $sum_f (type $sum_t) (param $x i32) (param $y i32) (result i32)\n"
        "    local.get $x\n"
        "    local.get $y\n"
        "    i32.add)\n"
        "  (export \"sum\" (func $sum_f)))";

    wasm_byte_vec_t wat;
    wasm_byte_vec_new(&wat, strlen(wat_string), wat_string);
    wasm_byte_vec_t wasm_bytes;
    wat2wasm(&wat, &wasm_bytes);
    wasm_byte_vec_delete(&wat);

    printf("Creating the store...\n");
    wasm_engine_t* engine = wasm_engine_new();
    wasm_store_t* store = wasm_store_new(engine);

    printf("Compiling module...\n");
    wasm_module_t* module = wasm_module_new(store, &wasm_bytes);

    if (!module) {
        printf("> Error compiling module!\n");
        return 1;
    }

    wasm_byte_vec_delete(&wasm_bytes);

    printf("Creating imports...\n");
    wasm_extern_vec_t import_object = WASM_EMPTY_VEC;

    printf("Instantiating module...\n");
    wasm_instance_t* instance = wasm_instance_new(store, module, &import_object, NULL);

    if (!instance) {
      printf("> Error instantiating module!\n");
      return 1;
    }

    printf("Retrieving exports...\n");
    wasm_extern_vec_t exports;
    wasm_instance_exports(instance, &exports);

    if (exports.size == 0) {
        printf("> Error accessing exports!\n");
        return 1;
    }

    printf("Retrieving the `sum` function...\n");
    wasm_func_t* sum_func = wasm_extern_as_func(exports.data[0]);

    if (sum_func == NULL) {
        printf("> Failed to get the `sum` function!\n");
        return 1;
    }

    printf("Calling `sum` function...\n");
    wasm_val_t args_val[2] = { WASM_I32_VAL(3), WASM_I32_VAL(4) };
    wasm_val_t results_val[1] = { WASM_INIT_VAL };
    wasm_val_vec_t args = WASM_ARRAY_VEC(args_val);
    wasm_val_vec_t results = WASM_ARRAY_VEC(results_val);

    if (wasm_func_call(sum_func, &args, &results)) {
        printf("> Error calling the `sum` function!\n");

        return 1;
    }

    printf("Results of `sum`: %d\n", results_val[0].of.i32);

    wasm_module_delete(module);
    wasm_extern_vec_delete(&exports);
    wasm_instance_delete(instance);
    wasm_store_delete(store);
    wasm_engine_delete(engine);
}
```

## Building

You can compile Wasmer shared library from source:

```text
make build-capi
```

This will generate the shared library \(depending on your system\):

* Windows: `target/release/libwasmer_c_api.dll`
* macOS: `target/release/libwasmer_runtime_c_api.dylib`
* Linux: `target/release/libwasmer_runtime_c_api.so`

If you want to generate the library and headers in a friendly format as shown in [Usage](#usage), you can execute the following in Wasmer root:

```bash
make package-capi
```

This command will generate a `package` directory, that you can then use easily in the [Wasmer C API examples](https://docs.wasmer.io/integrations/examples).


## Testing

Tests are run using the release build of the library.  If you make
changes or compile with non-default features, please ensure you
rebuild in release mode for the tests to see the changes.

To run all the full suite of tests, enter Wasmer root directory
and run the following commands:

```sh
$ make test-capi
```

## `wasmer config`

`wasmer config` output various configuration information needed to compile programs which use Wasmer.

### `wasmer config --pkg-config`

It outputs the necessary details for compiling and linking a program to Wasmer,
using the `pkg-config` format:

```bash
$ wasmer config --pkg-config > $PKG_CONFIG_PATH/wasmer.pc
```

### `wasmer config --includedir`

Directory containing Wasmer headers:

```bash
$ wasmer config --includedir
/users/myuser/.wasmer/include
```

### `wasmer config --libdir`

Directory containing Wasmer libraries:

```bash
$ wasmer config --libdir
/users/myuser/.wasmer/lib
```

### `wasmer config --libs`

Libraries needed to link against Wasmer components:

```bash
$ wasmer config --libs
-L/Users/myuser/.wasmer/lib -lwasmer
```

### `wasmer config --cflags`

Headers needed to build against Wasmer components:

```bash
$ wasmer config --cflags
-I/Users/myuser/.wasmer/include/wasmer
```

## License

Wasmer is primarily distributed under the terms of the [MIT
license][mit-license] ([LICENSE][license]).


[wasmer_h]: ./wasmer.h
[wasmer_hh]: ./wasmer.hh
[mit-license]: http://opensource.org/licenses/MIT
[license]: https://github.com/wasmerio/wasmer/blob/master/LICENSE
[Wasmer release page]: https://github.com/wasmerio/wasmer/releases
