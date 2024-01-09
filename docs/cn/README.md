<div align="center">
  <a href="https://wasmer.io" target="_blank" rel="noopener noreferrer">
    <img width="300" src="https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/logo.png" alt="Wasmer logo">
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
    <a href="https://slack.wasmer.io">
      <img src="https://img.shields.io/static/v1?label=Slack&message=join%20us!&color=brighgreen" alt="Slack channel">
    </a>
  </p>

  <h3>
    <a href="https://wasmer.io/">网站</a>
    <span> • </span>
    <a href="https://docs.wasmer.io">文档</a>
    <span> • </span>
    <a href="https://slack.wasmer.io/">聊天</a>
  </h3>

</div>

<br />

[Wasmer](https://wasmer.io/) 提供基于 [WebAssembly](https://webassembly.org/) 的超轻量级容器，其可以在任何地方运行：从桌面到云、以及 IoT 设备，并且也能嵌入到 [*任何编程语言*](https://github.com/wasmerio/wasmer#language-integrations) 中.

> 其他语言的 Readme: [🇩🇪 Deutsch-德語](https://github.com/wasmerio/wasmer/blob/master/docs/de/README.md) • [🇬🇧 English-英文](https://github.com/wasmerio/wasmer/blob/master/README.md) • [🇪🇸 Español-西班牙语](https://github.com/wasmerio/wasmer/blob/master/docs/es/README.md) • [🇫🇷 Français-法语](https://github.com/wasmerio/wasmer/blob/master/docs/fr/README.md) • [🇯🇵 日本語-日文](https://github.com/wasmerio/wasmer/blob/master/docs/ja/README.md).

## 特性

* **快速又安全**. Wasmer在完全沙盒化的环境中以“接近本机”的速度运行 WebAssembly。

* **可插拔**. Wasmer 可以根据你的需求支持不同的编译框架 (LLVM，Cranelift ...).

* **通用的**. 你可以在**任何平台**(macOS, Linux and Windows) 和芯片组运行 Wasmer.  

* **符合标准**. 运行时通过了[官方WebAssembly测试集](https://github.com/WebAssembly/testsuite) 支持[WASI](https://github.com/WebAssembly/WASI) 和[Emscripten](https://emscripten.org/).

## 快速开始

Wasmer 不需要安装其他依赖. 你可以使用以下安装程序进行安装:

```sh
curl https://get.wasmer.io -sSfL | sh
```

<details>
  <summary>使用Powershell (Windows)</summary>
  <p>

```powershell
iwr https://win.wasmer.io -useb | iex
```

</p>
</details>

> 有关更多安装选项，请参见 [wasmer-install](https://github.com/wasmerio/wasmer-install): Homebrew, Scoop, Cargo...


#### 执行WebAssembly文件

安装Wasmer之后，你已经准备好执行第一个WebAssembly文件了! 🎉

您可以通过运行 QuickJS 开始: [qjs.wasm](https://registry-cdn.wapm.io/contents/_/quickjs/0.0.3/build/qjs.wasm)

```bash
$ wasmer qjs.wasm
QuickJS - Type "\h" for help
qjs > const i = 1 + 2;
qjs > console.log("hello " + i);
hello 3
```

#### 接下来是你可以做的:

- [在你的Rust应用程序中使用Wasmer](https://docs.wasmer.io/integrations/rust)
- [在WAPM上发布Wasm程序包](https://docs.wasmer.io/ecosystem/wapm/publishing-your-package)
- [阅读有关Wasmer的更多信息](https://medium.com/wasmer/)

## 语言整合

📦 Wasmer 运行时能以库的形式**嵌入到不同的语言**，因此你可以在任何地方使用WebAssembly.

| &nbsp; | 语言 | 程序包 | 文档 |
|-|-|-|-|
| ![Rust logo] | [**Rust**][Rust integration] | [`wasmer` Rust crate] | [文档][rust docs]
| ![C logo] | [**C**][C integration] | [`wasm.h` header] | [文档][c docs] |
| ![C++ logo] | [**C++**][C integration] | [`wasm.hh` header] | [文档][c docs] |
| ![C# logo] | [**C#**][C# integration] | [`WasmerSharp` NuGet package] | [文档][c# docs] |
| ![D logo] | [**D**][D integration] | [`wasmer` Dub package] | [文档][d docs] |
| ![Python logo] | [**Python**][Python integration] | [`wasmer` PyPI package] | [文档][python docs] |
| ![JS logo] | [**Javascript**][JS integration] | [`@wasmerio` NPM packages] | [文档][js docs] |
| ![Go logo] | [**Go**][Go integration] | [`wasmer` Go package] | [文档][go docs] |
| ![PHP logo] | [**PHP**][PHP integration] | [`wasm` PECL package] | [文档][php docs] |
| ![Ruby logo] | [**Ruby**][Ruby integration] | [`wasmer` Ruby Gem] | [文档][ruby docs] |
| ![Java logo] | [**Java**][Java integration] | [`wasmer/wasmer-jni` Bintray package] | [文档][java docs] |
| ![Elixir logo] | [**Elixir**][Elixir integration] | [`wasmex` hex package] | [文档][elixir docs] |
| ![R logo] | [**R**][R integration] | *没有已发布的软件包* | [文档][r docs] |
| ![Postgres logo] | [**Postgres**][Postgres integration] | *没有已发布的软件包* | [文档][postgres docs] |
|  | [**Swift**][Swift integration] | *没有已发布的软件包* | |
| ![Zig logo] | [**Zig**][Zig integration] | *no published package* | |
| ![Ocaml logo] | [**OCaml**][OCaml integration] | [`wasmer` OCaml package] | |

[👋 缺少语言？](https://github.com/wasmerio/wasmer/issues/new?assignees=&labels=%F0%9F%8E%89+enhancement&template=---feature-request.md&title=)

[rust logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/rust.svg
[rust integration]: https://github.com/wasmerio/wasmer/tree/master/lib/api
[`wasmer` rust crate]: https://crates.io/crates/wasmer/
[rust docs]: https://docs.rs/wasmer/

[c logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/c.svg
[c integration]: https://github.com/wasmerio/wasmer/tree/master/lib/c-api
[`wasmer.h` headers]: https://wasmerio.github.io/wasmer/c/
[c docs]: https://docs.rs/wasmer-c-api/*/wasmer/wasm_c_api/index.html

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
[python docs]: https://github.com/wasmerio/wasmer-python#api-of-the-wasmer-extensionmodule

[go logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/go.svg
[go integration]: https://github.com/wasmerio/wasmer-go
[`wasmer` go package]: https://pkg.go.dev/github.com/wasmerio/wasmer-go/wasmer
[go docs]: https://pkg.go.dev/github.com/wasmerio/wasmer-go/wasmer?tab=doc

[php logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/php.svg
[php integration]: https://wasmerio.github.io/wasmer-php/
[`wasm` pecl package]: https://pecl.php.net/package/wasm
[php docs]: https://wasmerio.github.io/wasmer-php/wasm/

[js logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/js.svg
[js integration]: https://github.com/wasmerio/wasmer-js
[`@wasmerio` npm packages]: https://www.npmjs.com/org/wasmer
[js docs]: https://docs.wasmer.io/integrations/js/reference-api

[ruby logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/ruby.svg
[ruby integration]: https://github.com/wasmerio/wasmer-ruby
[`wasmer` ruby gem]: https://rubygems.org/gems/wasmer
[ruby docs]: https://www.rubydoc.info/gems/wasmer/

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

[OCaml logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/ocaml.svg
[OCaml integration]: https://github.com/wasmerio/wasmer-ocaml
[`wasmer` OCaml package]: https://opam.ocaml.org/packages/wasmer/

## 贡献

**我们欢迎任何形式的贡献，尤其是来自社区新成员的贡献** 💜

你可以在[我们的出色文档](https://docs.wasmer.io/developers/build-from-source) 中学习如何构建 Wasmer 运行时!

### 测试

想要测试吗?  [参考 Wasmer 文档](https://docs.wasmer.io/ecosystem/wasmer/building-from-source/testing).

## 社区

Wasmer 拥有一个由出色的开发人员和贡献者组成的社区。 欢迎你，请加入我们! 👋

### 频道

- [Slack](https://slack.wasmer.io/)
- [Twitter](https://twitter.com/wasmerio)
- [Facebook](https://www.facebook.com/wasmerio)
- [Email](mailto:hello@wasmer.io)
