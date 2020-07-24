<div align="center">
  <a href="https://wasmer.io" target="_blank" rel="noopener noreferrer">
    <img width="300" src="https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/logo.png" alt="Wasmer logo">
  </a>
  
  <p>
    <a href="https://dev.azure.com/wasmerio/wasmer/_build/latest?definitionId=3&branchName=master">
      <img src="https://img.shields.io/azure-devops/build/wasmerio/wasmer/3.svg?style=flat-square" alt="Build Status">
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

[Wasmer](https://wasmer.io/) is a standalone [WebAssembly](https://webassembly.org/) runtime:

- **Universal**: Wasmer is available in _Linux, macOS and Windows_ (for both Desktop and [ARM](https://medium.com/wasmer/running-webassembly-on-arm-7d365ed0e50c))
- **Fast**: Wasmer aims to run WebAssembly at near-native speed
- **Pluggable**: Wasmer can be used from almost **any programming language**
- **Safe**: supporting [WASI](https://github.com/WebAssembly/WASI) and [Emscripten](https://emscripten.org/)

It is used to run software fast, universally and safely: standalone applications and universal libraries.

## Quickstart

#### 1. Install Wasmer (_more installation methods are also [available](https://github.com/wasmerio/wasmer-install)_)

```sh
curl https://get.wasmer.io -sSfL | sh
```

<details>
  <summary>With PowerShell</summary>
  <p>

```powershell
iwr https://win.wasmer.io -useb | iex
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

📦 Wasmer runtime can be used as a library **embedded in different languages**, so you can use WebAssembly _anywhere_.

| &nbsp; | Language | Package | Docs |
|-|-|-|-|
| ![Rust logo] | [**Rust**][Rust integration] | [`wasmer` Rust crate] | [Docs][rust docs]
| ![C logo] | [**C/C++**][C integration] | [`wasmer.h` headers] | [Docs][c docs] |
| ![C# logo] | [**C#**][C# integration] | [`WasmerSharp` NuGet package] | [Docs][c# docs] |
| ![Python logo] | [**Python**][Python integration] | [`wasmer` PyPI package] | [Docs][python docs] |
| ![JS logo] | [**Javascript**][JS integration] | [`@wasmerio` NPM packages] | [Docs][js docs] |
| ![Go logo] | [**Go**][Go integration] | [`wasmer` Go package] | [Docs][go docs] |
| ![PHP logo] | [**PHP**][PHP integration] | [`wasm` PECL package] | [Docs][php docs] |
| ![Ruby logo] | [**Ruby**][Ruby integration] | [`wasmer` Ruby Gem] | [Docs][ruby docs] |
| ![Java logo] | [**Java**][Java integration] | [`wasmer/wasmer-jni` Bintray package] | [Docs][java docs] |
| ![Elixir logo] | [*Elixir**][Elixir integration] | | [Docs][elixir docs] |
| ![R logo] | [**R**][R integration] | | [Docs][r docs] |
| ![Postgres logo] | [**Postgres**][Postgres integration] | | |

[👋 Missing a language?](https://github.com/wasmerio/wasmer/issues/new?assignees=&labels=%F0%9F%8E%89+enhancement&template=---feature-request.md&title=)

[rust logo]: ./assets/languages/rust.svg
[rust integration]: https://github.com/wasmerio/wasmer-rust-example
[`wasmer` rust crate]: https://crates.io/crates/wasmer/
[rust docs]: https://wasmerio.github.io/wasmer/crates/wasmer_runtime

[c logo]: ./assets/languages/c.svg
[c integration]: https://github.com/wasmerio/wasmer-c-api
[`wasmer.h` headers]: https://wasmerio.github.io/wasmer/c/runtime-c-api/
[c docs]: https://wasmerio.github.io/wasmer/c/runtime-c-api/

[c# logo]: ./assets/languages/csharp.svg
[c# integration]: https://github.com/migueldeicaza/WasmerSharp
[`wasmersharp` nuget package]: https://www.nuget.org/packages/WasmerSharp/
[c# docs]: https://migueldeicaza.github.io/WasmerSharp/

[python logo]: ./assets/languages/python.svg
[python integration]: https://github.com/wasmerio/wasmer-c-api
[`wasmer` pypi package]: https://pypi.org/project/wasmer/
[python docs]: https://github.com/wasmerio/python-ext-wasm#api-of-the-wasmer-extensionmodule

[go logo]: ./assets/languages/go.svg
[go integration]: https://github.com/wasmerio/go-ext-wasm
[`wasmer` go package]: https://pkg.go.dev/github.com/wasmerio/go-ext-wasm/wasmer
[go docs]: https://pkg.go.dev/github.com/wasmerio/go-ext-wasm/wasmer?tab=doc

[php logo]: ./assets/languages/php.svg
[php integration]: https://github.com/wasmerio/php-ext-wasm
[`wasm` pecl package]: https://pecl.php.net/package/wasm
[php docs]: https://wasmerio.github.io/php-ext-wasm/wasm/

[js logo]: ./assets/languages/js.svg
[js integration]: https://github.com/wasmerio/wasmer-js
[`@wasmerio` npm packages]: https://www.npmjs.com/org/wasmer
[js docs]: https://docs.wasmer.io/wasmer-js/wasmer-js

[ruby logo]: ./assets/languages/ruby.svg
[ruby integration]: https://github.com/wasmerio/ruby-ext-wasm
[`wasmer` ruby gem]: https://rubygems.org/gems/wasmer
[ruby docs]: https://www.rubydoc.info/gems/wasmer/

[java logo]: ./assets/languages/java.svg
[java integration]: https://github.com/wasmerio/java-ext-wasm
[`wasmer/wasmer-jni` bintray package]: https://bintray.com/wasmer/wasmer-jni/wasmer-jni
[java docs]: https://github.com/wasmerio/java-ext-wasm/#api-of-the-wasmer-library

[elixir logo]: ./assets/languages/elixir.svg
[elixir integration]: https://github.com/tessi/wasmex
[elixir docs]: https://hexdocs.pm/wasmex/api-reference.html

[r logo]: ./assets/languages/r.svg
[r integration]: https://github.com/dirkschumacher/wasmr
[r docs]: https://github.com/dirkschumacher/wasmr#example

[postgres logo]: ./assets/languages/postgres.svg
[postgres integration]: https://github.com/wasmerio/postgres-ext-wasm

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
