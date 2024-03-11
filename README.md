<div align="center">
  <a href="https://wasmer.io" target="_blank">
    <picture>
      <source srcset="https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/logo-white.png"  media="(prefers-color-scheme: dark)">
      <img width="300" src="https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/logo.png" alt="Wasmer logo">
    </picture>
  </a>

  <p>
    <a href="https://github.com/wasmerio/wasmer/actions?query=workflow%3Abuild">
      <img src="https://github.com/wasmerio/wasmer/actions/workflows/build.yml/badge.svg?event=push" alt="Build Status">
    </a>
    <a href="https://github.com/wasmerio/wasmer/blob/master/LICENSE">
      <img src="https://img.shields.io/github/license/wasmerio/wasmer.svg" alt="License">
    </a>
    <a href="https://docs.wasmer.io">
      <img src="https://img.shields.io/static/v1?label=Docs&message=docs.wasmer.io&color=blue" alt="Wasmer Docs">
    </a>
    <a href="https://discord.gg/rWkMNStrEW">
      <img src="https://img.shields.io/discord/1110300506942881873" alt="Wasmer on Discord">
    </a>
  </p>
</div>

<br />

Wasmer is a _blazing fast_ and _secure_ [**WebAssembly**](https://webassembly.org) runtime that enables incredibly
_lightweight containers_ to run anywhere: from _Desktop_ to the _Cloud_, _Edge_ and your browser.

- **Secure** by default. No file, network, or environment access, unless explicitly enabled.
- **Pluggable**. supports [**WASIX**](https://wasix.org/), [WASI](https://github.com/WebAssembly/WASI) and [Emscripten](https://emscripten.org/) out of the box.
- **Incredibly Fast**. Run WebAssembly at near-native speeds.
- **Embeddable** [anywhere via Wasmer SDKs](https://github.com/wasmerio/wasmer/#wasmer-sdk)

### Install Wasmer

```sh
curl https://get.wasmer.io -sSfL | sh
```

<details>
  <summary>Other installation options (Powershell, Brew, Cargo, ...)</summary>
  
  _Wasmer can be installed from various package managers. Choose the one that fits best for your environment:_
  
  * Powershell (Windows)
    ```powershell
    iwr https://win.wasmer.io -useb | iex
    ```

- <a href="https://formulae.brew.sh/formula/wasmer">Homebrew</a> (macOS, Linux)

  ```sh
  brew install wasmer
  ```

- <a href="https://github.com/ScoopInstaller/Main/blob/master/bucket/wasmer.json">Scoop</a> (Windows)

  ```sh
  scoop install wasmer
  ```

- <a href="https://chocolatey.org/packages/wasmer">Chocolatey</a> (Windows)

  ```sh
  choco install wasmer
  ```

- <a href="https://crates.io/crates/cargo-binstall/">Cargo binstall</a>

  ```sh
  cargo binstall wasmer-cli
  ```

- <a href="https://crates.io/crates/wasmer-cli/">Cargo</a>

  _Note: All the available
  features are described in the [`wasmer-cli`
  crate docs](https://github.com/wasmerio/wasmer/tree/master/lib/cli/README.md)_

  ```sh
  cargo install wasmer-cli
  ```

> Looking for more installation options? See [the `wasmer-install`
> repository](https://github.com/wasmerio/wasmer-install) to learn
> more!

</details>

> Note: You can also try Wasmer online in [wasmer.sh](https://wasmer.sh/)
> 
### Quickstart

You can start by running
[Cowsay](https://wasmer.io/syrusakbary/cowsay):

```bash
$ wasmer run cowsay "hello world"
 _____________
< hello world >
 -------------
        \   ^__^
         \  (oo)\_______
            (__)\       )\/\
               ||----w |
                ||     ||
```

> There are many more available packages, such as [`wasmer/python`](https://wasmer.io/wasmer/python) or [`quickjs`](https://wasmer.io/saghul/quickjs). [Create your own package](https://docs.wasmer.io/registry/get-started), or explore packages from the community: https://wasmer.io/explore

#### Here is what you can do next:

- [Run a package](https://docs.wasmer.io/runtime/get-started)
- [Publish a package](https://docs.wasmer.io/registry/get-started)
- [Deploy your website](https://docs.wasmer.io/edge/get-started)
- [Read more about Wasmer](https://wasmer.io/posts)

## Wasmer SDK

You can use the Wasmer runtime **embedded in different
languages** with the Wasmer SDK:

|                  | Language                             | Package                               | Documentation          |
| ---------------- | ------------------------------------ | ------------------------------------- | ---------------------- |
| ![Rust logo]     | [**Rust**][Rust integration]         | [`wasmer` Rust crate]                 | [Learn][rust docs]     |
| ![C logo]        | [**C**][C integration]               | [`wasm.h` header]                     | [Learn][c docs]        |
| ![C++ logo]      | [**C++**][C integration]             | [`wasm.hh` header]                    | [Learn][c docs]        |
| ![C# logo]       | [**C#**][C# integration]             | [`WasmerSharp` NuGet package]         | [Learn][c# docs]       |
| ![D logo]        | [**D**][D integration]               | [`wasmer` Dub package]                | [Learn][d docs]        |
| ![Python logo]   | [**Python**][Python integration]     | [`wasmer` PyPI package]               | [Learn][python docs]   |
| ![JS logo]       | [**Javascript**][JS integration]     | [`@wasmerio` NPM packages]            | [Learn][js docs]       |
| ![Go logo]       | [**Go**][Go integration]             | [`wasmer` Go package]                 | [Learn][go docs]       |
| ![PHP logo]      | [**PHP**][PHP integration]           | [`wasm` PECL package]                 | [Learn][php docs]      |
| ![Ruby logo]     | [**Ruby**][Ruby integration]         | [`wasmer` Ruby Gem]                   | [Learn][ruby docs]     |
| ![Java logo]     | [**Java**][Java integration]         | [`wasmer/wasmer-jni` Bintray package] | [Learn][java docs]     |
| ![R logo]        | [**R**][R integration]               | _no published package_                | [Learn][r docs]        |
| ![Postgres logo] | [**Postgres**][Postgres integration] | _no published package_                | [Learn][postgres docs] |
| ![Swift logo]    | [**Swift**][Swift integration]       | _no published package_                |                        |
| ![Zig logo]      | [**Zig**][Zig integration]           | _no published package_                |                        |
| ![Dart logo]     | [**Dart**][Dart integration]         | [`wasm` pub package]                  |                        |
| ![Crystal logo]  | [**Crystal**][Crystal integration]   | _no published package_                | [Learn][crystal docs]  |
| ![Lisp logo]     | [**Lisp**][Lisp integration]         | _no published package_                |                        |
| ![Julia logo]    | [**Julia**][Julia integration]       | _no published package_                |                        |
| ![VLang logo]    | [**V**][vlang integration]           | _no published package_                |                        |
| ![Ocaml logo]    | [**OCaml**][OCaml integration]       | [`wasmer` OCaml package]              |                        |

[👋&nbsp;&nbsp;Missing a language?](https://github.com/wasmerio/wasmer/issues/new?assignees=&labels=%F0%9F%8E%89+enhancement&template=---feature-request.md&title=)

[rust logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/rust.svg
[rust integration]: https://github.com/wasmerio/wasmer/tree/master/lib/api
[`wasmer` rust crate]: https://crates.io/crates/wasmer/
[rust docs]: https://docs.rs/wasmer/
[c logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/c.svg
[c integration]: https://github.com/wasmerio/wasmer/tree/master/lib/c-api
[`wasm.h` header]: https://github.com/wasmerio/wasmer/blob/master/lib/c-api/tests/wasm-c-api/include/wasm.h
[c docs]: https://docs.rs/wasmer-c-api/*/wasmer/wasm_c_api/index.html
[c++ logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/cpp.svg
[`wasm.hh` header]: https://github.com/wasmerio/wasmer/blob/master/lib/c-api/tests/wasm-c-api/include/wasm.hh
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
[python docs]: https://wasmerio.github.io/wasmer-python/api/wasmer
[go logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/go.svg
[go integration]: https://github.com/wasmerio/wasmer-go
[`wasmer` go package]: https://pkg.go.dev/github.com/wasmerio/wasmer-go/wasmer
[go docs]: https://pkg.go.dev/github.com/wasmerio/wasmer-go/wasmer?tab=doc
[php logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/php.svg
[php integration]: https://github.com/wasmerio/wasmer-php
[`wasm` pecl package]: https://pecl.php.net/package/wasm
[php docs]: https://wasmerio.github.io/wasmer-php/
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
[swift logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/swift.svg
[swift integration]: https://github.com/AlwaysRightInstitute/SwiftyWasmer
[zig logo]: https://raw.githubusercontent.com/ziglang/logo/master/zig-favicon.png
[zig integration]: https://github.com/zigwasm/wasmer-zig
[dart logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/dart.svg
[dart integration]: https://github.com/dart-lang/wasm
[`wasm` pub package]: https://pub.dev/packages/wasm
[lisp logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/lisp.svg
[lisp integration]: https://github.com/helmutkian/cl-wasm-runtime
[crystal logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/crystal.svg
[crystal integration]: https://github.com/naqvis/wasmer-crystal
[crystal docs]: https://naqvis.github.io/wasmer-crystal/
[julia logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/julia.svg
[julia integration]: https://github.com/Pangoraw/Wasmer.jl
[vlang logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/vlang.svg
[vlang integration]: https://github.com/vlang/wasmer
[OCaml logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/ocaml.svg
[OCaml integration]: https://github.com/wasmerio/wasmer-ocaml
[`wasmer` OCaml package]: https://opam.ocaml.org/packages/wasmer/

## Contribute

We appreciate your help! 💜

We recommend reading the following guide on how to contribute into a complex project successfully:
https://mitchellh.com/writing/contributing-to-complex-projects

Check our docs on how to [build Wasmer from
source](https://docs.wasmer.io/developers/build-from-source) or [test your changes](https://docs.wasmer.io/developers/testing).

<!-- ## Bounties

For some issues we offer paid bounties. You'll get paid automatically as soon as your PR solving the issue is merged!

<a href="https://console.algora.io/org/wasmerio/bounties?status=open">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://console.algora.io/api/og/wasmerio/bounties.png?p=0&status=open&theme=dark">
    <img alt="Bounties of wasmerio" src="https://console.algora.io/api/og/wasmerio/bounties.png?p=0&status=open&theme=light">
  </picture>
</a> -->

## Community

Wasmer has an amazing community of developers and contributors. Welcome, please join us! 👋

- [Wasmer Community Discord](https://discord.gg/rWkMNStrEW)
- [Wasmer on Twitter](https://twitter.com/wasmerio)

--------

> _README also in:
> [🇨🇳 中 文 -Chinese](https://github.com/wasmerio/wasmer/blob/master/docs/cn/README.md) •
> [🇩🇪 Deutsch-German](https://github.com/wasmerio/wasmer/blob/master/docs/de/README.md) •
> [🇪🇸 Español-Spanish](https://github.com/wasmerio/wasmer/blob/master/docs/es/README.md) •
> [🇫🇷 Français-French](https://github.com/wasmerio/wasmer/blob/master/docs/fr/README.md) •
> [🇯🇵 日本 語 -Japanese](https://github.com/wasmerio/wasmer/blob/master/docs/ja/README.md) •
> [🇰🇷 한국어 -Korean](https://github.com/wasmerio/wasmer/blob/master/docs/ko/README.md)_.

