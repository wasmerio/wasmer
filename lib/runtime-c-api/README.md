# Wasmer Runtime C API


## Generating header files
1. `cargo install cbindgen`
2. `cbindgen lib/runtime-c-api/ -o lib/runtime-c-api/wasmer.h`