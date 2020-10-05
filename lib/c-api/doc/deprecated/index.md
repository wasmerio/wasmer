# Wasmer Runtime C API

[Wasmer] is a standalone WebAssembly runtime for running WebAssembly [outside of the browser](https://webassembly.org/docs/non-web/), supporting [WASI](https://github.com/WebAssembly/WASI) and [Emscripten](https://emscripten.org/).

The Wasmer Runtime C API exposes a C and a C++ API to interact
with the Wasmer Runtime, so you can use WebAssembly anywhere.

[Wasmer]: https://github.com/wasmerio/wasmer
[WebAssembly]: https://webassembly.org/

# Usage

The shared object files and the headers will
be automatically available **inside the Wasmer installed path**.

> Please check the following docs to see how to [install Wasmer in your system](https://github.com/wasmerio/wasmer#1-install-wasmer).

The C ([`wasmer.h`][wasmer_h]) and C++ ([`wasmer.hh`][wasmer_hh]) header
files can be found in the Wasmer `include` directory:

```bash
wasmer config --includedir
```

The runtime shared libraries (`.so`, `.dylib`, `.dll`) can be found in the Wasmer
`lib` directory:

```bash
wasmer config --libdir
```

> Note: You can also download the libraries or header files directly
from [Wasmer release page].

The full C API documentation can be found here:
https://wasmerio.github.io/wasmer/c-api/

Here is a simple example to use the C API:

```c
#include <stdio.h>
#include "wasmer.h"
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

# Examples

You can check more examples of how to use the Wasmer C API here:

https://docs.wasmer.io/integrations/c/examples

## pkg-config

The Wasmer binary ships with an utility tool that outputs config
in the `pkg-config` format.

You can use it like:

```bash
wasmer config --pkg-config > $PKG_CONFIG_PATH/wasmer.pc
```

# License

Wasmer is primarily distributed under the terms of the [MIT
license][mit-license] ([LICENSE][license]).


[wasmer_h]: https://wasmerio.github.io/wasmer/c-api/wasmer_8h.html
[wasmer_hh]: https://wasmerio.github.io/wasmer/c-api/wasmer_8hh.html
[mit-license]: http://opensource.org/licenses/MIT
[license]: https://github.com/wasmerio/wasmer/blob/master/LICENSE
[Wasmer release page]: https://github.com/wasmerio/wasmer/releases
