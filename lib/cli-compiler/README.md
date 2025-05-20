# `wasmer-cli-compiler` 
[![Build Status](https://github.com/wasmerio/wasmer/actions/workflows/build.yml/badge.svg?style=flat-square)](https://github.com/wasmerio/wasmer/actions?query=workflow%3Abuild) [![Join Wasmer Slack](https://img.shields.io/static/v1?label=Slack&message=join%20chat&color=brighgreen&style=flat-square)](https://slack.wasmer.io) [![MIT License](https://img.shields.io/github/license/wasmerio/wasmer.svg?style=flat-square)](https://github.com/wasmerio/wasmer/blob/main/LICENSE)

This crate is the Wasmer Compiler only CLI.


## Features

The Compiler only Wasmer supports the following features:
* `wasi` (default): support for [WASI].
* `singlepass`: support for the [Singlepass compiler].

[WASI]: https://github.com/wasmerio/wasmer/tree/main/lib/wasi/
[Singlepass compiler]: https://github.com/wasmerio/wasmer/tree/main/lib/compiler-singlepass/

## CLI commands

Once you have Wasmer installed, you can start executing WebAssembly files easily:

Get the current Wasmer version:

```bash
wasmer-compiler -V
```

Compile a WebAssembly file:

```bash
wasmer-compiler compile myfile.wasm -o myfile.wasmu --singlepass
```
