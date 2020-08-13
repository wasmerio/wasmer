# Wasmer Integration tests

All Wasmer end to end integration tests live here.
We have different kind of integration tests:

## CLI Integration tests

This tests check that the `wasmer` CLI works as it should when running it
as a Command in a shell, for each of the supported compilers.

## C Integration tests

This tests verify that Wasmer wasm-c-api tests are passing for each of the
supported compilers.

## Rust Integration tests

This tests verify that the `wasmer` API fulfill the required API that
external users use.
