
<div align="center">
  <a href="https://wasmer.io" target="_blank" rel="noopener noreferrer">
    <img width="300" src="https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/logo.png" alt="Wasmer logo">
  </a>
  
  <p>
    <a href="https://dev.azure.com/wasmerio/wasmer/_build/latest?definitionId=3&branchName=master">
      <img src="https://img.shields.io/azure-devops/build/wasmerio/wasmer/3.svg?style=flat-square" alt="Build Status">
    </a>
    <a href="https://slack.wasmer.io">
      <img src="https://img.shields.io/static/v1?label=Slack&message=join%20chat&color=brighgreen&style=flat-square" alt="Slack channel">
    </a> 
    <a href="https://github.com/wasmerio/wasmer/blob/master/LICENSE">
      <img src="https://img.shields.io/github/license/wasmerio/wasmer.svg?style=flat-square" alt="License">
    </a>
  </p>

  <h3>
    <a href="https://wasmer.io/">Website</a>
    <span> • </span>
    <a href="https://docs.wasmer.io">Docs</a>
    <span> • </span>
    <a href="https://medium.com/wasmer/">Blog</a>
    <span> • </span>
    <a href="https://slack.wasmer.io/">Slack</a>
    <span> • </span>
    <a href="https://twitter.com/wasmerio">Twitter</a>
  </h3>

</div>

<br />

[Wasmer](https://wasmer.io/) is a standalone [WebAssembly](https://webassembly.org/) runtime:
* **Universal**: Wasmer is available in *Linux, macOS and Windows* (for both Desktop and [ARM](https://medium.com/wasmer/running-webassembly-on-arm-7d365ed0e50c))
* **Fast**: Wasmer aims to run WebAssembly at near-native speed
* **Pluggable**: Wasmer can be used from almost **any programming language**
* **Safe**: supporting [WASI](https://github.com/WebAssembly/WASI) and [Emscripten](https://emscripten.org/)

It is used to run software fast, universally and safely: standalone applications and universal libraries.

## Contents

- [Quickstart](#quickstart)
- [Language Integrations](#language-integrations)
- [Contribute](#contribute)
- [Community](#community)

## Quickstart

Get started with Wasmer:

#### 1. Install Wasmer

```sh
curl https://get.wasmer.io -sSfL | sh
```
> Note: *Wasmer is also [available on Windows](https://github.com/wasmerio/wasmer/releases)*

<details>
  <summary><b>Alternative</b>: Install with Homebrew</summary>
  <p>

```sh
brew install wasmer
```

</p>
</details>

#### 2. Use Wasmer

Download a WASM file, and use it universally! You can start with QuickJS: [qjs.wasm](https://registry-cdn.wapm.io/contents/_/quickjs/0.0.3/build/qjs.wasm)

```bash
wasmer qjs.wasm
```

#### 3. Next steps

Here is what you can do next:

- [Use Wasmer from your Rust application](https://docs.wasmer.io/integrations/rust)
- [Publish a Wasm package on WAPM](https://docs.wasmer.io/ecosystem/wapm/publishing-your-package)
- [Read more about Wasmer](https://medium.com/wasmer/)


### Language Integrations

Wasmer runtime can be used as a library embedded in different languages, so you can **use WebAssembly anywhere** 🎉

| &nbsp; | Language | Docs | Author(s) | Maintenance | Release | Stars |
|-|-|-|-|-|-|-|
| ![Rust logo](./assets/languages/rust.svg) | [**Rust**](https://github.com/wasmerio/wasmer-rust-example) | [Docs](https://wasmerio.github.io/wasmer/crates/wasmer_runtime/) | Wasmer | actively developed | <a href="https://crates.io/crates/wasmer-runtime/" target="_blank">![last release](https://img.shields.io/crates/v/wasmer-runtime?style=flat-square)</a> | ![number of Github stars](https://img.shields.io/github/stars/wasmerio/wasmer?style=flat-square) |
| ![C logo](./assets/languages/c.svg) | [**C/C++**](https://github.com/wasmerio/wasmer-c-api) | [Docs](https://wasmerio.github.io/wasmer/c/runtime-c-api/) | Wasmer | actively developed | <a href="https://github.com/wasmerio/wasmer-c-api/" target="_blank">![last release](https://img.shields.io/github/v/release/wasmerio/wasmer?style=flat-square)</a> | ![number of Github stars](https://img.shields.io/github/stars/wasmerio/wasmer?style=flat-square) |
| ![Python logo](./assets/languages/python.svg) | [**Python**](https://github.com/wasmerio/python-ext-wasm) | [Docs](https://github.com/wasmerio/python-ext-wasm#api-of-the-wasmer-extensionmodule) | Wasmer | actively developed | <a href="https://pypi.org/project/wasmer/" target="_blank">![last release](https://img.shields.io/pypi/v/wasmer?style=flat-square)</a> | ![number of Github stars](https://img.shields.io/github/stars/wasmerio/python-ext-wasm?style=flat-square) |
| ![Go logo](./assets/languages/go.svg) | [**Go**](https://github.com/wasmerio/go-ext-wasm) | [Docs](https://github.com/wasmerio/go-ext-wasm#basic-example-exported-function) | Wasmer | actively developed | <a href="https://github.com/wasmerio/go-ext-wasm" target="_blank">![last release](https://img.shields.io/github/v/release/wasmerio/go-ext-wasm?style=flat-square)</a> | ![number of Github stars](https://img.shields.io/github/stars/wasmerio/go-ext-wasm?style=flat-square) |
| ![PHP logo](./assets/languages/php.svg) | [**PHP**](https://github.com/wasmerio/php-ext-wasm) | [Docs](https://wasmerio.github.io/php-ext-wasm/wasm/) | Wasmer | actively developed | <a href="https://pecl.php.net/package/wasm" target="_blank">![last release](https://img.shields.io/github/v/release/wasmerio/php-ext-wasm?style=flat-square)</a> | ![number of Github stars](https://img.shields.io/github/stars/wasmerio/php-ext-wasm?style=flat-square) |
| ![Ruby logo](./assets/languages/ruby.svg) | [**Ruby**](https://github.com/wasmerio/ruby-ext-wasm) | [Docs](https://www.rubydoc.info/gems/wasmer/) | Wasmer | actively developed | <a href="https://rubygems.org/gems/wasmer" target="_blank">![last release](https://img.shields.io/gem/v/wasmer?style=flat-square)</a> | ![number of Github stars](https://img.shields.io/github/stars/wasmerio/ruby-ext-wasm?style=flat-square) |
| ![Postgres logo](./assets/languages/postgres.svg) | [**Postgres**](https://github.com/wasmerio/postgres-ext-wasm) | |  Wasmer | actively developed | <a href="https://github.com/wasmerio/postgres-ext-wasm" target="_blank">![last release](https://img.shields.io/github/v/release/wasmerio/postgres-ext-wasm?style=flat-square)</a> | ![number of Github stars](https://img.shields.io/github/stars/wasmerio/postgres-ext-wasm?style=flat-square) |
| ![JS Logo](./assets/languages/js.svg) | [**JavaScript**](https://github.com/wasmerio/wasmer-js) | [Docs](https://docs.wasmer.io/wasmer-js/wasmer-js) | Wasmer | actively developed | <a href="https://www.npmjs.com/package/@wasmer/wasi" target="_blank">![last release](https://img.shields.io/npm/v/@wasmer/wasi?style=flat-square)</a> | ![number of Github stars](https://img.shields.io/github/stars/wasmerio/wasmer-js?style=flat-square) |
| ![C# logo](./assets/languages/csharp.svg) | [**C#/.Net**](https://github.com/migueldeicaza/WasmerSharp) | [Docs](https://migueldeicaza.github.io/WasmerSharp/) |[Miguel de Icaza](https://github.com/migueldeicaza) | actively developed | <a href="https://www.nuget.org/packages/WasmerSharp/" target="_blank">![last release](https://img.shields.io/nuget/v/WasmerSharp?style=flat-square)</a> | ![number of Github stars](https://img.shields.io/github/stars/migueldeicaza/WasmerSharp?style=flat-square) |
| ![R logo](./assets/languages/r.svg) | [**R**](https://github.com/dirkschumacher/wasmr) | [Docs](https://github.com/dirkschumacher/wasmr#example) | [Dirk Schumacher](https://github.com/dirkschumacher) | actively developed | | ![number of Github stars](https://img.shields.io/github/stars/dirkschumacher/wasmr?style=flat-square) |
| ![Elixir logo](./assets/languages/elixir.png) | [**Elixir**](https://github.com/tessi/wasmex) | [Docs](https://hexdocs.pm/wasmex/api-reference.html) | [Philipp Tessenow](https://github.com/tessi) | actively developed | <a href="https://hex.pm/packages/wasmex" target="_blank">![last release](https://img.shields.io/hexpm/v/wasmex?style=flat-square)</a> | ![number of Github stars](https://img.shields.io/github/stars/tessi/wasmex?style=flat-square) |
| ❓ | [your language is missing?](https://github.com/wasmerio/wasmer/issues/new?assignees=&labels=%F0%9F%8E%89+enhancement&template=---feature-request.md&title=) | | | | |


## Contribute

**We welcome any form of contribution, especially from new members of our community** 💜

You can check how to build the Wasmer runtime in [our awesome docs](https://docs.wasmer.io/ecosystem/wasmer/building-from-source)!

### Testing

Test you want? The [Wasmer docs will show you how](https://docs.wasmer.io/ecosystem/wasmer/building-from-source/testing).

## Community

Wasmer has an amazing community developers and contributors. Welcome, please join us! 👋

### Channels

- [Slack](https://slack.wasmer.io/)
- [Twitter](https://twitter.com/wasmerio)
- [Facebook](https://www.facebook.com/wasmerio)
- [Email](mailto:hello@wasmer.io)
