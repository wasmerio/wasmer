# The WASIX Conformance Test Suite

A suite of tests used by WebAssembly runtimes to ensure they conform with
[WASIX][wasix].

## Architecture

This test suite is split up into several sub-folders,

- `suite` - the actual test suite, where each test is its own executable under
  the `src/bin/` folder
- `shared` - shared types and utilities used for defining WASIX conformance tests
- `harness` - a test runner which will discover all tests and run them

[wasix]: https://wasix.org/
