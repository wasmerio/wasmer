# WASI test suite

WASI test files with expected output in a custom WAST format.

## Setup

In order to run the test generator properly you will need:

- `rustup` installed and on your PATH
- `wasm-opt` from `binaryen` and `wasm-strip` from `wabt` are installed and on your PATH

## Usage

```
Positional arguments:
  free                    if you want to specify specific tests to generate

Optional arguments:
  -a, --all-versions      Whether or not to do operations for all versions of WASI or just the latest.
  -g, --generate-wasm     Whether or not the Wasm will be generated.
  -s, --set-up-toolchain  Whether or not the logic to install the needed Rust compilers is run.
  -h, --help              Print the help message
```

And here's an example of how to generate these tests:

```bash
cargo run -- -as # set up the toolchains for all targets
cargo run -- -ag # generate the WASI tests for all targets
```

If you want to generate specific tests (it's faster when you're developing) you can use this command:

```bash
cargo run -- -g fd_rename_path # If you want to run the test in fd_rename_path.rs
```

## Updating in Wasmer

Run
`git subtree pull --prefix tests/wasi-wast git@github.com:wasmerio/wasi-tests master --squash`
