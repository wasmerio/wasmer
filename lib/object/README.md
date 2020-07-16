# Wasmer Native Object

The Wasmer Native Object crate aims at cross-generating native objects
for various platforms.

This crate is the foundation of [the `wasmer-engine-native`
crate](../engine-native/). Given a compilation result, i.e. the result
of `wasmer_compiler::Compiler::compile_module`, this crate exposes
functions to create an `Object` file for a given target. It is a
useful thin layer on top of [the `object`
crate](https://github.com/gimli-rs/object).
