# Wasmer Runtime C API

## Generating header files
Run `make capi` from wasmer project root directory

## Running tests
The tests can be run via `cargo test`, E.g. `cargo test -p wasmer-runtime-c-api -- --nocapture`

*Running manually*
`cmake . && make && make test` from the `lib/runtime-c-api/tests` directory
