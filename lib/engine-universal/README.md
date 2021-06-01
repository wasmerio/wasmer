<div align="center">
  <a href="https://wasmer.io" target="_blank" rel="noopener noreferrer">
    <img width="300" src="https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/logo.png" alt="Wasmer logo">
  </a>

  <h1>The <code>wasmer-engine-universal</code> library</h1>

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
    <a href="https://crates.io/crates/wasmer-engine-universal">
      <img src="https://img.shields.io/crates/v/wasmer-engine-universal.svg?style=flat-square" alt="crates.io" />
    </a>
    <a href="https://wasmerio.github.io/wasmer/crates/wasmer_engine_universal/">
      <img src="https://img.shields.io/badge/documentation-read-informational?style=flat-square" alt="documentation" />
    </a>
  </p>
</div>

<br />

The Wasmer Universal engine is usable with any compiler implementation based
on [`wasmer-compiler`]. After the compiler process the result, the Universal
pushes it into memory and links its contents so it can be usable by
the [`wasmer`] API.

*Note: you can find a [full working example using the Universal engine
here][example].*

### Acknowledgments

This project borrowed some of the code of the code memory and unwind
tables from the [`wasmtime-jit`], the code since then has evolved
significantly.

Please check [Wasmer `ATTRIBUTIONS`] to further see licenses and other
attributions of the project.


[`wasmer-compiler`]: https://github.com/wasmerio/wasmer/tree/master/lib/compiler
[`wasmer`]: https://github.com/wasmerio/wasmer/tree/master/lib/api
[example]: https://github.com/wasmerio/wasmer/blob/master/examples/engine_universal.rs
[`wasmtime-jit`]: https://crates.io/crates/wasmtime-jit
[Wasmer `ATTRIBUTIONS`]: https://github.com/wasmerio/wasmer/blob/master/ATTRIBUTIONS.md
