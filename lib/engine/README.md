<div align="center">
  <a href="https://wasmer.io" target="_blank" rel="noopener noreferrer">
    <img width="300" src="https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/logo.png" alt="Wasmer logo">
  </a>

  <h1>The <code>wasmer-engine</code> library</h1>

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
    <a href="https://crates.io/crates/wasmer-engine">
      <img src="https://img.shields.io/crates/v/wasmer-engine.svg?style=flat-square" alt="crates.io" />
    </a>
    <a href="https://wasmerio.github.io/wasmer/crates/wasmer_engine/">
      <img src="https://img.shields.io/badge/documentation-read-informational?style=flat-square" alt="documentation" />
    </a>
  </p>
</div>

<br />

This crate is the general abstraction for creating Engines in Wasmer.

Wasmer Engines are mainly responsible for two things:
* Transform the compilation code (from any Wasmer Compiler) to
  **create** an `Artifact`,
* **Load** an`Artifact` so it can be used by the user (normally,
  pushing the code into executable memory and so on).

It currently has three implementations:

1. Universal with [`wasmer-engine-universal`],
2. Native with [`wasmer-engine-dylib`],
3. Object with [`wasmer-engine-staticlib`].

## Example Implementation

Please check [`wasmer-engine-dummy`] for an example implementation for
an `Engine`.

### Acknowledgments

This project borrowed some of the code of the trap implementation from
the [`wasmtime-api`], the code since then has evolved significantly.

Please check [Wasmer `ATTRIBUTIONS`] to further see licenses and other
attributions of the project.


[`wasmer-engine-universal`]: https://github.com/wasmerio/wasmer/tree/master/lib/engine-universal
[`wasmer-engine-dylib`]: https://github.com/wasmerio/wasmer/tree/master/lib/engine-dylib
[`wasmer-engine-staticlib`]: https://github.com/wasmerio/wasmer/tree/master/lib/engine-staticlib
[`wasmer-engine-dummy`]: https://github.com/wasmerio/wasmer/tree/master/tests/lib/engine-dummy
[`wasmtime-api`]: https://crates.io/crates/wasmtime
[Wasmer `ATTRIBUTIONS`]: https://github.com/wasmerio/wasmer/blob/master/ATTRIBUTIONS.md
