# Wasmer Integration tests

All Wasmer end to end integration tests live here.
We have different kind of integration tests:

## CLI Integration tests

This tests check that the `wasmer` CLI works as it should when running it
as a Command in a shell, for each of the supported compilers.

### Snapshot Tests

A snapshot test suite is located at `./cli/tests/snapshot.rs`.

The file contains tests that run various Webassembly files through `wasmer run`.
The output is stored as snapshots in the repository.

The [insta](https://github.com/mitsuhiko/insta) crate is used for snapshots.

#### Working With Snapshots

* Install the cargo-insta CLI
  `cargo install cargo-insta`
* Update snapshots:
  ```
  cd ./cli/
  cargo insta test --review -- snapshot
  ```

## C Integration tests

This tests verify that Wasmer wasm-c-api tests are passing for each of the
supported compilers.

## Rust Integration tests

This tests verify that the `wasmer` API fulfill the required API that
external users use.
