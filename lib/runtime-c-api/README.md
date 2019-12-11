<p align="center">
  <a href="https://wasmer.io" target="_blank" rel="noopener noreferrer">
    <img width="300" src="https://raw.githubusercontent.com/wasmerio/wasmer/master/logo.png" alt="Wasmer logo">
  </a>
</p>

<p align="center">
  <a href="https://dev.azure.com/wasmerio/wasmer/_build/latest?definitionId=3&branchName=master">
    <img src="https://img.shields.io/azure-devops/build/wasmerio/wasmer/3.svg?style=flat-square" alt="Build Status">
  </a>
  <a href="https://github.com/wasmerio/wasmer/blob/master/LICENSE">
    <img src="https://img.shields.io/github/license/wasmerio/wasmer.svg?style=flat-square" alt="License">
  </a>
  <a href="https://spectrum.chat/wasmer">
    <img src="https://withspectrum.github.io/badge/badge.svg" alt="Join the Wasmer Community">
  </a>
  <a href="https://crates.io/crates/wasmer-runtime-c-api">
    <img src="https://img.shields.io/crates/d/wasmer-runtime-c-api.svg?style=flat-square" alt="Number of downloads from crates.io">
  </a>
  <a href="https://docs.rs/wasmer-runtime-c-api">
    <img src="https://docs.rs/wasmer-runtime-c-api/badge.svg" alt="Read our API documentation">
  </a>
</p>

# Wasmer Runtime C API

Wasmer is a standalone JIT WebAssembly runtime, aiming to be fully
compatible with WASI, Emscripten, Rust and Go. [Learn
more](https://github.com/wasmerio/wasmer).

This crate exposes a C and a C++ API for the Wasmer runtime.

# Usage

The C and C++ header files can be found in the source tree of this
crate, respectively [`wasmer.h`][wasmer_h] and
[`wasmer.hh`][wasmer_hh]. They are automatically generated, and always
up-to-date in this repository.
The runtime shared library (so, dll, dylib) can also be downloaded in Wasmer [release page](https://github.com/wasmerio/wasmer/releases).

Here is a simple example to use the C API:

```c
#include <stdio.h>
#include "../wasmer.h"
#include <assert.h>
#include <stdint.h>

int main()
{
    // Read the Wasm file bytes.
    FILE *file = fopen("sum.wasm", "r");
    fseek(file, 0, SEEK_END);
    long len = ftell(file);
    uint8_t *bytes = malloc(len);
    fseek(file, 0, SEEK_SET);
    fread(bytes, 1, len, file);
    fclose(file);

    // Prepare the imports.
    wasmer_import_t imports[] = {};

    // Instantiate!
    wasmer_instance_t *instance = NULL;
    wasmer_result_t instantiation_result = wasmer_instantiate(&instance, bytes, len, imports, 0);

    assert(instantiation_result == WASMER_OK);

    // Let's call a function.
    // Start by preparing the arguments.

    // Value of argument #1 is `7i32`.
    wasmer_value_t argument_one;
    argument_one.tag = WASM_I32;
    argument_one.value.I32 = 7;

    // Value of argument #2 is `8i32`.
    wasmer_value_t argument_two;
    argument_two.tag = WASM_I32;
    argument_two.value.I32 = 8;

    // Prepare the arguments.
    wasmer_value_t arguments[] = {argument_one, argument_two};

    // Prepare the return value.
    wasmer_value_t result_one;
    wasmer_value_t results[] = {result_one};

    // Call the `sum` function with the prepared arguments and the return value.
    wasmer_result_t call_result = wasmer_instance_call(instance, "sum", arguments, 2, results, 1);

    // Let's display the result.
    printf("Call result:  %d\n", call_result);
    printf("Result: %d\n", results[0].value.I32);

    // `sum(7, 8) == 15`.
    assert(results[0].value.I32 == 15);
    assert(call_result == WASMER_OK);

    wasmer_instance_destroy(instance);

    return 0;
}
```

# Testing

Tests are run using the release build of the library.  If you make
changes or compile with non-default features, please ensure you
rebuild in release mode for the tests to see the changes.

The tests can be run via `cargo test`, such as:

```sh
$ cargo test --release -- --nocapture
```

To run tests manually, enter the `lib/runtime-c-api/tests` directory
and run the following commands:

```sh
$ cmake .
$ make
$ make test
```


# License

Wasmer is primarily distributed under the terms of the [MIT
license][mit-license] ([LICENSE][license]).


[wasmer_h]: ./wasmer.h
[wasmer_hh]: ./wasmer.hh
[mit-license]: http://opensource.org/licenses/MIT
[license]: https://github.com/wasmerio/wasmer/blob/master/LICENSE
