# WASI plugin example

In this example we extend the imports of Wasmer's WASI ABI to demonstrate how custom plugins work.

See the `wasmer/examples/plugin.rs` file for the source code of the host system.

## Compiling

```
# Install an up to date version of Rust nightly
# Add the target
rustup target add wasm32-unknown-wasi
# build it
cargo build --release --target=wasm32-unknown-wasi
# copy it to examples folder
cp ../../target/wasm32-unknown-wasi/release/plugin-for-example.wasm ../
```

## Running
```
# Go back to top level Wasmer dir
cd ..
# Run the example
cargo run --example plugin
```

## Explanation

In this example, we instantiate a system with an extended (WASI)[wasi] ABI, allowing our program to rely on Wasmer's implementation of the syscalls defined by WASI as well as our own that we made.  This allows us to use the full power of an existing ABI, like WASI, and give it super-powers for our specific use case.

Because the Rust WASI doesn't support the crate type of `cdylib`, we have to include a main function which we don't use.

[wasi]: https://hacks.mozilla.org/2019/03/standardizing-wasi-a-webassembly-system-interface/