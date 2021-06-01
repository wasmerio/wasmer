<div align="center">
  <a href="https://wasmer.io" target="_blank" rel="noopener noreferrer">
    <img width="300" src="https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/logo.png" alt="Wasmer logo">
  </a>

  <h1>The <code>wasmer-object</code> library</h1>

  <p>
    <a href="https://github.com/wasmerio/wasmer/actions?query=workflow%3Abuild">
      <img src="https://github.com/wasmerio/wasmer/workflows/build/badge.svg?style=flat-square" alt="Build Status" />
    </a>
    <a href="https://github.com/wasmerio/wasmer/blob/master/LICENSE">
      <img src="https://img.shields.io/github/license/wasmerio/wasmer.svg?style=flat-square" alt="License" />
    </a>
    <a href="https://slack.wasmer.io">
      <img src="https://img.shields.io/static/v1?label=Slack&message=join%20chat&color=brighgreen&style=flat-square" alt="Slack channel" />
    </a>
    <a href="https://crates.io/crates/wasmer-object">
      <img src="https://img.shields.io/crates/v/wasmer-object.svg?style=flat-square" alt="crates.io" />
    </a>
    <a href="https://wasmerio.github.io/wasmer/crates/wasmer_object/">
      <img src="https://img.shields.io/badge/documentation-read-informational?style=flat-square" alt="documentation" />
    </a>
  </p>
</div>

<br />

The Wasmer Native Object crate aims at cross-generating native objects
for various platforms.

This crate is the foundation of [the `wasmer-engine-dylib`
crate](../engine-dylib/). Given a compilation result, i.e. the result
of `wasmer_compiler::Compiler::compile_module`, this crate exposes
functions to create an `Object` file for a given target. It is a
useful thin layer on top of [the `object`
crate](https://github.com/gimli-rs/object).
