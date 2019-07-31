# WASI plugin example

In this example we extend the imports of Wasmer's WASI ABI to demonstrate how custom plugins work.

See the `wasmer/examples/plugin.rs` file for the source code of the host system.

## Compiling
_Attention Windows users: WASI target only works with the `nightly-x86_64-pc-windows-gnu` toolchain._ 
```
# Install an up to date version of Rust nightly
# Add the target
rustup target add wasm32-unknown-wasi
# build it
cargo +nightly build --release --target=wasm32-unknown-wasi
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

## Inspecting the plugin
```
# Install wabt via wapm; installed globally with the `g` flag
wapm install -g wabt
# Turn the binary WASM file in to a readable WAT text file
wapm run wasm2wat examples/plugin-for-example.wasm
```

At the top of the file we can see which functions this plugin expects.  Most are covered by WASI, but we handle the rest.

## Explanation

In this example, we instantiate a system with an extended (WASI)[wasi] ABI, allowing our program to rely on Wasmer's implementation of the syscalls defined by WASI as well as our own that we made.  This allows us to use the full power of an existing ABI, like WASI, and give it super-powers for our specific use case.

Because the Rust WASI doesn't support the crate type of `cdylib`, we have to include a main function which we don't use.  This is being discussed [here](https://github.com/WebAssembly/WASI/issues/24).

[wasi]: https://hacks.mozilla.org/2019/03/standardizing-wasi-a-webassembly-system-interface/
