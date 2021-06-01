<div align="center">
  <a href="https://wasmer.io" target="_blank" rel="noopener noreferrer">
    <img width="300" src="https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/logo.png" alt="Wasmer logo">
  </a>

  <h1>The <code>wasmer-types</code> library</h1>

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
    <a href="https://crates.io/crates/wasmer-types">
      <img src="https://img.shields.io/crates/v/wasmer-types.svg?style=flat-square" alt="crates.io" />
    </a>
    <a href="https://wasmerio.github.io/wasmer/crates/wasmer_types/">
      <img src="https://img.shields.io/badge/documentation-read-informational?style=flat-square" alt="documentation" />
    </a>
  </p>
</div>

<br />

This library provides all the types and traits necessary to use
WebAssembly easily anywhere.

Amongst other things, it defines the following _types_:

* `units` like `Pages` or `Bytes`,
* `types` and `values` like `I32`, `I64`, `F32`, `F64`, `ExternRef`,
  `FuncRef`, `V128`, value conversions, `ExternType`, `FunctionType`
  etc.,
* `native` contains a set of trait and implementations to deal with
  WebAssembly types that have a direct representation on the host,
* `memory_view`, an API to read/write memories when bytes are
  interpreted as particular types (`i8`, `i16`, `i32` etc.),
* `indexes` contains all the possible WebAssembly module indexes for
  various types,
* `initializers` for tables, data etc.,
* `features` to enable or disable some WebAssembly features inside the
  Wasmer runtime,
* etc.

### Acknowledgments

This project borrowed some of the code for the entity structure from
[the `cranelift-entity`
crate](https://crates.io/crates/cranelift-entity).  We decided to move
it here to help on serialization/deserialization and also to ease the
integration with other tools like
[`loupe`](https://github.com/wasmerio/loupe/).

Please check [Wasmer
`ATTRIBUTIONS`](https://github.com/wasmerio/wasmer/blob/master/ATTRIBUTIONS.md)
to further see licenses and other attributions of the project.
