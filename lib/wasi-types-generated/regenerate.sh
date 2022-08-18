#!/bin/bash

rm src/bindings.rs
wit-bindgen wasmer --import wit/wasi-snapshot0.wit wit/wasi-filesystem.wit --out-dir src/
#wit-bindgen rust-wasm --import wit/wasi-snapshot0.wit wit/wasi-filesystem.wit --out-dir .
