<div align="center">
  <a href="https://wasmer.io" target="_blank" rel="noopener noreferrer">
    <img width="300" src="https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/logo.png" alt="Wasmer logo">
  </a>

  <h1>The <code>wasmer-wasi-experimental-io-devices</code> library</h1>

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
    <a href="https://crates.io/crates/wasmer-wasi-experimental-io-devices">
      <img src="https://img.shields.io/crates/v/wasmer-wasi-experimental-io-devices.svg?style=flat-square" alt="crates.io" />
    </a>
    <a href="https://wasmerio.github.io/wasmer/crates/wasmer_wasi_experimental_io_devices/">
      <img src="https://img.shields.io/badge/documentation-read-informational?style=flat-square" alt="documentation" />
    </a>
  </p>
</div>

<br />

This library is an experimental extension of WebAssembly System
Interface (WASI) for basic graphics. To learn more about WASI, check
our
[`wasmer-wasi`](https://github.com/wasmerio/wasmer/tree/master/lib/wasi)
library.

I/O devices is not part of the WASI standard yet. Hence the
_experimental_ status of this library.

The only real introduction to this library is in our article [Building
Graphical Applications with Wasmer and
WASI](https://medium.com/wasmer/wasmer-io-devices-announcement-6f2a6fe23081). It
introduces the need for a framebuffer abstraction for WASI.

You may also want to check the
[`io-devices-lib`](https://github.com/wasmerio/io-devices-lib) project
that contains libraries for interacting with this Wasmer Experimental
IO Devices library.

<figure>
  <img src="https://miro.medium.com/max/700/1*D8nEQ_eJ5S6iOov8u0gdpQ.gif" />

  <figcaption>Demonstration of
  <a href="https://wapm.io/package/torch2424/wasmerboy"><code>wasmerboy</code></a>,
  a game boy emulator written for WebAssembly using AssemblyScript,
  built for Wasmer using the Experimental I/O Devices</figcaption>
</figure>
