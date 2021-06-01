<div align="center">
  <a href="https://wasmer.io" target="_blank" rel="noopener noreferrer">
    <img width="300" src="https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/logo.png" alt="Wasmer logo">
  </a>

  <p>
    <a href="https://github.com/wasmerio/wasmer/actions?query=workflow%3Abuild">
      <img src="https://github.com/wasmerio/wasmer/workflows/build/badge.svg?style=flat-square" alt="Build Status">
    </a>
    <a href="https://github.com/wasmerio/wasmer/blob/master/LICENSE">
      <img src="https://img.shields.io/github/license/wasmerio/wasmer.svg?style=flat-square" alt="License">
    </a>
    <a href="https://slack.wasmer.io">
      <img src="https://img.shields.io/static/v1?label=Slack&message=join%20chat&color=brighgreen&style=flat-square" alt="Slack channel">
    </a>
  </p>

  <h3>
    <a href="https://wasmer.io/">Website</a>
    <span> • </span>
    <a href="https://docs.wasmer.io">Docs</a>
    <span> • </span>
    <a href="https://slack.wasmer.io/">Chat</a>
  </h3>

</div>

<br />

[Wasmer](https://wasmer.io/) is a runtime that enables super
lightweight containers based on [WebAssembly](https://webassembly.org)
to run anywhere: from Desktop to the Cloud and IoT devices, and also
embedded in [*numerous programming
language*](https://github.com/wasmerio/wasmer#language-integrations).

_This document is also available in:
[🇨🇳 中 文 -Chinese](https://github.com/wasmerio/wasmer/blob/master/docs/cn/README.md),
[🇪🇸 Español-Spanish](https://github.com/wasmerio/wasmer/blob/master/docs/es/README.md),
[🇫🇷 Français-French](https://github.com/wasmerio/wasmer/blob/master/docs/fr/README.md),
[🇯🇵 日本 語 -Japanese](https://github.com/wasmerio/wasmer/blob/master/docs/ja/README.md)_.

## ✨ Features

* **Fast & Safe**. Wasmer runs WebAssembly at _near-native_ speed in a
  fully sandboxed environment.

* **Pluggable**. To best suit your needs, Wasmer supports different
  compilation strategies (_aka_ the compilers — based on LLVM, based
  on Cranelift, or Singlepass) and artifact strategies (_aka_ the
  engines — Universal, Dylib, Staticlib).

* **Universal**. You can run Wasmer on any _platform_ (Linux, macOS
  and Windows) and _chipset_.

* **Standards compliant**. The runtime passes [official WebAssembly
  test suite](https://github.com/WebAssembly/testsuite) supporting
  [WASI](https://github.com/WebAssembly/WASI) and
  [Emscripten](https://emscripten.org/).

## 🏁 Quickstart

The quickest way to get fun with Wasmer is to install its CLI. It
ships with no dependency. Let's first start by installing it, then
let's see how to execute a WebAssembly file.

### Installing the Wasmer CLI

Wasmer can be installed from various package managers, scripts, or
built from sources… Pick what is best for you:

* <details>
    <summary>With <code>curl</code></summary>

    This is kind of the universal way to install Wasmer. If you don't
    trust this approach, please see other installation options.

    ```sh
    curl https://get.wasmer.io -sSfL | sh
    ```

  </details>

* <details>
    <summary>With PowerShell</summary>

    This installation process is dedicated to Windows users:

    ```powershell
    iwr https://win.wasmer.io -useb | iex
    ```

  </details>

* <details>
    <summary>With <a href="https://formulae.brew.sh/formula/wasmer">Homebrew</a></summary>

    Homebrew is mainly a package manager for macOS:

    ```sh
    brew install wasmer
    ```

  </details>

* <details>
    <summary>With <a href="https://github.com/ScoopInstaller/Main/blob/master/bucket/wasmer.json">Scoop</a></summary>

    Scoop is a package manager for Windows:

    ```sh
    scoop install wasmer
    ```

  </details>

* <details>
    <summary>With <a href="https://chocolatey.org/packages/wasmer">Chocolatey</a></summary>

    Chocolatey is a package manager for Windows:

    ```sh
    choco install wasmer
    ```

  </details>

* <details>
    <summary>With <a href="https://crates.io/crates/wasmer-cli/">Cargo</a></summary>

    Cargo is the crate installer for Rust.

    The following command will install `wasmer-cli`. All the available
    features are described in the [`wasmer-cli`
    documentation](https://github.com/wasmerio/wasmer/tree/master/lib/cli/README.md).

    ```sh
    cargo install wasmer-cli
    ```

  </details>

* <details>
    <summary>From source</summary>

    Inside the root of this repository (in this case, you're likely to
    need some dependencies):

    ```sh
    make build-wasmer
    ```

    [Read the
    documentation](https://docs.wasmer.io/ecosystem/wasmer/building-from-source)
    to learn more about this approach.

  </details>

* More installation options? See [the `wasmer-install`
  repository](https://github.com/wasmerio/wasmer-install) to learn
  more!

### Executing a WebAssembly file

After installing Wasmer you should be ready to execute your first WebAssembly file! 🎉

You can start by running
[QuickJS](https://github.com/bellard/quickjs/), which is a small and
embeddable Javascript engine, compiled as a WebAssembly module,
[`qjs.wasm`](https://registry-cdn.wapm.io/contents/_/quickjs/0.0.3/build/qjs.wasm):

```bash
$ wasmer qjs.wasm
QuickJS - Type "\h" for help
qjs > const i = 1 + 2;
qjs > console.log("hello " + i);
hello 3
```

### Discover

Here are some clues about what you can do next:

- [Use Wasmer from your Rust application](https://docs.wasmer.io/integrations/rust),
- [Publish a Wasm package on WAPM](https://docs.wasmer.io/ecosystem/wapm/publishing-your-package),
- [Read more about Wasmer](https://medium.com/wasmer/).

## 📦 Language Integrations

The Wasmer runtime can be used as a library **embedded in different
languages**, so you can use WebAssembly _anywhere_.

| | Language | Package | Documentation |
|-|-|-|-|
| ![Rust logo] | [**Rust**][Rust integration] | [`wasmer` Rust crate] | [Learn][rust docs]
| ![C logo] | [**C/C++**][C integration] | [`wasmer_wasm.h` header] | [Learn][c docs] |
| ![C# logo] | [**C#**][C# integration] | [`WasmerSharp` NuGet package] | [Learn][c# docs] |
| ![D logo] | [**D**][D integration] | [`wasmer` Dub package] | [Learn][d docs] |
| ![Python logo] | [**Python**][Python integration] | [`wasmer` PyPI package] | [Learn][python docs] |
| ![JS logo] | [**Javascript**][JS integration] | [`@wasmerio` NPM packages] | [Learn][js docs] |
| ![Go logo] | [**Go**][Go integration] | [`wasmer` Go package] | [Learn][go docs] |
| ![PHP logo] | [**PHP**][PHP integration] | [`wasm` PECL package] | [Learn][php docs] |
| ![Ruby logo] | [**Ruby**][Ruby integration] | [`wasmer` Ruby Gem] | [Learn][ruby docs] |
| ![Java logo] | [**Java**][Java integration] | [`wasmer/wasmer-jni` Bintray package] | [Learn][java docs] |
| ![Elixir logo] | [**Elixir**][Elixir integration] | [`wasmex` hex package] | [Learn][elixir docs] |
| ![R logo] | [**R**][R integration] | *no published package* | [Learn][r docs] |
| ![Postgres logo] | [**Postgres**][Postgres integration] | *no published package* | [Learn][postgres docs] |
|  | [**Swift**][Swift integration] | *no published package* | |
| ![Zig logo] | [**Zig**][Zig integration] | *no published package* | |

[👋 Missing a language?](https://github.com/wasmerio/wasmer/issues/new?assignees=&labels=%F0%9F%8E%89+enhancement&template=---feature-request.md&title=)

[rust logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/rust.svg
[rust integration]: https://github.com/wasmerio/wasmer/tree/master/lib/api
[`wasmer` rust crate]: https://crates.io/crates/wasmer/
[rust docs]: https://wasmerio.github.io/wasmer/crates/wasmer

[c logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/c.svg
[c integration]: https://github.com/wasmerio/wasmer/tree/master/lib/c-api
[`wasmer_wasm.h` header]: https://github.com/wasmerio/wasmer/blob/master/lib/c-api/wasmer_wasm.h
[c docs]: https://wasmerio.github.io/wasmer/crates/wasmer_c_api

[c# logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/csharp.svg
[c# integration]: https://github.com/migueldeicaza/WasmerSharp
[`wasmersharp` nuget package]: https://www.nuget.org/packages/WasmerSharp/
[c# docs]: https://migueldeicaza.github.io/WasmerSharp/

[d logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/d.svg
[d integration]: https://github.com/chances/wasmer-d
[`wasmer` Dub package]: https://code.dlang.org/packages/wasmer
[d docs]: https://chances.github.io/wasmer-d

[python logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/python.svg
[python integration]: https://github.com/wasmerio/wasmer-python
[`wasmer` pypi package]: https://pypi.org/project/wasmer/
[python docs]: https://wasmerio.github.io/wasmer-python/api/wasmer/

[go logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/go.svg
[go integration]: https://github.com/wasmerio/wasmer-go
[`wasmer` go package]: https://pkg.go.dev/github.com/wasmerio/wasmer-go/wasmer
[go docs]: https://pkg.go.dev/github.com/wasmerio/wasmer-go/wasmer?tab=doc

[php logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/php.svg
[php integration]: https://github.com/wasmerio/wasmer-php
[`wasm` pecl package]: https://pecl.php.net/package/wasm
[php docs]: https://wasmerio.github.io/wasmer-php/wasm/

[js logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/js.svg
[js integration]: https://github.com/wasmerio/wasmer-js
[`@wasmerio` npm packages]: https://www.npmjs.com/org/wasmer
[js docs]: https://docs.wasmer.io/integrations/js/reference-api

[ruby logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/ruby.svg
[ruby integration]: https://github.com/wasmerio/wasmer-ruby
[`wasmer` ruby gem]: https://rubygems.org/gems/wasmer
[ruby docs]: https://wasmerio.github.io/wasmer-ruby/wasmer_ruby/index.html

[java logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/java.svg
[java integration]: https://github.com/wasmerio/wasmer-java
[`wasmer/wasmer-jni` bintray package]: https://bintray.com/wasmer/wasmer-jni/wasmer-jni
[java docs]: https://github.com/wasmerio/wasmer-java/#api-of-the-wasmer-library

[elixir logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/elixir.svg
[elixir integration]: https://github.com/tessi/wasmex
[elixir docs]: https://hexdocs.pm/wasmex/api-reference.html
[`wasmex` hex package]: https://hex.pm/packages/wasmex

[r logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/r.svg
[r integration]: https://github.com/dirkschumacher/wasmr
[r docs]: https://github.com/dirkschumacher/wasmr#example

[postgres logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/postgres.svg
[postgres integration]: https://github.com/wasmerio/wasmer-postgres
[postgres docs]: https://github.com/wasmerio/wasmer-postgres#usage--documentation

[swift integration]: https://github.com/AlwaysRightInstitute/SwiftyWasmer

[zig logo]: https://raw.githubusercontent.com/ziglang/logo/master/zig-favicon.png
[zig integration]: https://github.com/zigwasm/wasmer-zig

## 🤲 Contribute

We welcome any form of contribution, especially from new members of
our community 💜.  You can check [how to build the Wasmer runtime
documentation from
sources](https://docs.wasmer.io/ecosystem/wasmer/building-from-source)!

### Testing

Test you want? The [Wasmer docs will show you
how](https://docs.wasmer.io/ecosystem/wasmer/building-from-source/testing).

## 👐 Community

Wasmer has an amazing community of developers and contributors. You are very welcome! 👋

### Channels

- [Community Slack workspace](https://slack.wasmer.io/),
- [Official Twitter account](https://twitter.com/wasmerio),
- [Official Facebook acount](https://www.facebook.com/wasmerio),
- [Email](mailto:hello@wasmer.io).
