# `wasmer-wasi-types` [![Build Status](https://github.com/wasmerio/wasmer/workflows/build/badge.svg?style=flat-square)](https://github.com/wasmerio/wasmer/actions?query=workflow%3Abuild) [![Join Wasmer Slack](https://img.shields.io/static/v1?label=Slack&message=join%20chat&color=brighgreen&style=flat-square)](https://slack.wasmer.io) [![MIT License](https://img.shields.io/github/license/wasmerio/wasmer.svg?style=flat-square)](https://github.com/wasmerio/wasmer/blob/master/LICENSE) [![crates.io](https://img.shields.io/crates/v/wasmer-wasi-types.svg)](https://crates.io/crates/wasmer-wasi-types)

This crate contains the WASI types necessary for `wasmer-wasi`. Please check this crate to learn more!

---

Run `regenerate.sh` to regenerate the wasi-types from
the `wasi-clean/typenames.wit` into the final Rust bindings.

The `wasi-types-generator-extra` generates some extra code
that wit-bindgen currently can't provide.