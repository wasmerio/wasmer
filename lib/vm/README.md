<div align="center">
  <a href="https://wasmer.io" target="_blank" rel="noopener noreferrer">
    <img width="300" src="https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/logo.png" alt="Wasmer logo">
  </a>

  <h1>The <code>wasmer-vm</code> library</h1>

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
    <a href="https://crates.io/crates/wasmer-vm">
      <img src="https://img.shields.io/crates/v/wasmer-vm.svg?style=flat-square" alt="crates.io" />
    </a>
    <a href="https://wasmerio.github.io/wasmer/crates/wasmer_vm/">
      <img src="https://img.shields.io/badge/documentation-read-informational?style=flat-square" alt="documentation" />
    </a>
  </p>
</div>

<br />

This crate contains the Wasmer VM runtime library, supporting the Wasm ABI used by any [`wasmer-engine`] implementation.

[`wasmer-engine`]: https://crates.io/crates/wasmer-engine

### Acknowledgments

This project borrowed some of the code for the VM structure and trapping from the [wasmtime-runtime](https://crates.io/crates/wasmtime-runtime).

Please check [Wasmer ATTRIBUTIONS](https://github.com/wasmerio/wasmer/blob/master/ATTRIBUTIONS.md) to further see licenses and other attributions of the project.
