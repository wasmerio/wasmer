<div align="center">
  <a href="https://wasmer.io" target="_blank" rel="noopener noreferrer">
    <img width="300" src="https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/logo.png" alt="Wasmer logo">
  </a>

  <p>
    <a href="https://github.com/wasmerio/wasmer/actions?query=workflow%3Abuild">
      <img src="https://github.com/wasmerio/wasmer/workflows/build/badge.svg?style=flat-square" alt="Build Status">
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
</div>

<br />

Wasmer는 _초경량 컨테이너_ 를 *Desktop*에서부터 *Cloud*, *Edge*, *IoT* 기기들까지 어디에서나 실행할 수 있는 _빠르고 안전한_ [**WebAssembly**](https://webassembly.org) 런타임 입니다.

> _이 문서는 아래와 같은 언어들을 지원합니다.:
[🇨🇳 中 文 -Chinese](https://github.com/wasmerio/wasmer/blob/master/docs/cn/README.md) • 
[🇩🇪 Deutsch-German](https://github.com/wasmerio/wasmer/blob/master/docs/de/README.md) • 
[🇪🇸 Español-Spanish](https://github.com/wasmerio/wasmer/blob/master/docs/es/README.md) • 
[🇫🇷 Français-French](https://github.com/wasmerio/wasmer/blob/master/docs/fr/README.md) • 
[🇯🇵 日本 語 -Japanese](https://github.com/wasmerio/wasmer/blob/master/docs/ja/README.md)_.
[🇰🇷 한국어 -Korean](https://github.com/wasmerio/wasmer/blob/master/docs/ko/README.md)_.

### 특징

* 기본적으로 안전합니다. 명시적으로 설정하지 않는 한 파일, 네트워크 또는 환경에 액세스할 수 없습니다.
* [WASI](https://github.com/WebAssembly/WASI)와 [Emscripten](https://emscripten.org/)을 즉시 지원합니다.
* 빠릅니다. native에 가까운 속도로 WebAssembly를 실행합니다.
* [여러 프로그래밍 언어](https://github.com/wasmerio/wasmer/#-language-integrations)에 임베디드 가능합니다.
* 최신 WebAssembly 제안(SIMD, Reference Types, Threads, ...)을 준수합니다.

### 설치

Wasmer CLI는 종속성이 없는 단일 실행 파일로 제공됩니다.

```sh
curl https://get.wasmer.io -sSfL | sh
```


<details>
  <summary>다른 설치 옵션 (Powershell, Brew, Cargo, ...)</summary>
  
  _Wasmer는 다양한 패키지 매니저를 통해 설치 할 수 있습니다. 환경에 가장 적합한 것을 선택하십시오.:_
  
  * Powershell (Windows)
    ```powershell
    iwr https://win.wasmer.io -useb | iex
    ```

  * <a href="https://formulae.brew.sh/formula/wasmer">Homebrew</a> (macOS, Linux)

    ```sh
    brew install wasmer
    ```

  * <a href="https://github.com/ScoopInstaller/Main/blob/master/bucket/wasmer.json">Scoop</a> (Windows)

    ```sh
    scoop install wasmer
    ```

  * <a href="https://chocolatey.org/packages/wasmer">Chocolatey</a> (Windows)

    ```sh
    choco install wasmer
    ```
  
  * <a href="https://crates.io/crates/wasmer-cli/">Cargo</a>

    _Note: 사용 가능한 모든 기능은 [`wasmer-cli`
    crate docs](https://github.com/wasmerio/wasmer/tree/master/lib/cli/README.md) 문서에 설명되어 있습니다._

    ```sh
    cargo install wasmer-cli
    ```

  > 더 많은 설치 옵션을 찾고 계십니까? 자세한 내용은 [the `wasmer-install`
  repository](https://github.com/wasmerio/wasmer-install)를 참조하십시오!
</details>

### 빠른 시작

WebAssembly 모듈([`qjs.wasm`](https://registry-cdn.wapm.io/contents/_/quickjs/0.0.3/build/qjs.wasm))로 컴파일된
작고 포함 가능한 Javascript 엔진인 [QuickJS](https://github.com/bellard/quickjs/)를 실행하여 시작할 수 있습니다.:

```bash
$ wasmer qjs.wasm
QuickJS - Type "\h" for help
qjs > const i = 1 + 2;
qjs > console.log("hello " + i);
hello 3
```

#### 다음에 할 수 있는 일 :

- [어플리케이션에서 wasmer 사용](https://docs.wasmer.io/integrations/rust)
- [WAPM에 wasm 패키지 게시](https://docs.wasmer.io/ecosystem/wapm/publishing-your-package)
- [Wasmer에 대해 자세히 알아보기](https://medium.com/wasmer/)

## 📦 다른 언어와의 통합

Wasmer 런타임은 **다른 언어에 내장된** 라이브러리로 사용할 수 있으므로 _어디에서나_ WebAssembly를 사용할 수 있습니다.

| | Language | Package | Documentation |
|-|-|-|-|
| ![Rust logo] | [**Rust**][Rust integration] | [`wasmer` Rust crate] | [Learn][rust docs]
| ![C logo] | [**C/C++**][C integration] | [`wasmer.h` header] | [Learn][c docs] |
| ![C# logo] | [**C#**][C# integration] | [`WasmerSharp` NuGet package] | [Learn][c# docs] |
| ![D logo] | [**D**][D integration] | [`wasmer` Dub package] | [Learn][d docs] |
| ![Python logo] | [**Python**][Python integration] | [`wasmer` PyPI package] | [Learn][python docs] |
| ![JS logo] | [**Javascript**][JS integration] | [`@wasmerio` NPM packages] | [Learn][js docs] |
| ![Go logo] | [**Go**][Go integration] | [`wasmer` Go package] | [Learn][go docs] |
| ![PHP logo] | [**PHP**][PHP integration] | [`wasm` PECL package] | [Learn][php docs] |
| ![Ruby logo] | [**Ruby**][Ruby integration] | [`wasmer` Ruby Gem] | [Learn][ruby docs] |
| ![Java logo] | [**Java**][Java integration] | [`wasmer/wasmer-jni` Bintray package] | [Learn][java docs] |
| ![Elixir logo] | [**Elixir**][Elixir integration] | [`wasmex` hex package] | [Learn][elixir docs] |
| ![R logo] | [**R**][R integration] | *공개 패키지 없음* | [Learn][r docs] |
| ![Postgres logo] | [**Postgres**][Postgres integration] | *공개 패키지 없음* | [Learn][postgres docs] |
|  | [**Swift**][Swift integration] | *공개 패키지 없음* | |
| ![Zig logo] | [**Zig**][Zig integration] | *공개 패키지 없음* | |
| ![Dart logo] | [**Dart**][Dart integration] | [`wasm` pub package] | |
|  | [**Lisp**][Lisp integration] | *under heavy development - no published package* | |
| ![Ocaml logo] | [**OCaml**][OCaml integration] | [`wasmer` OCaml package] | |

[👋&nbsp;&nbsp;없는 언어가 있습니까?](https://github.com/wasmerio/wasmer/issues/new?assignees=&labels=%F0%9F%8E%89+enhancement&template=---feature-request.md&title=)

[rust logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/rust.svg
[rust integration]: https://github.com/wasmerio/wasmer/tree/master/lib/api
[`wasmer` rust crate]: https://crates.io/crates/wasmer/
[rust docs]: https://docs.rs/wasmer/

[c logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/c.svg
[c integration]: https://github.com/wasmerio/wasmer/tree/master/lib/c-api
[`wasmer.h` header]: https://github.com/wasmerio/wasmer/blob/master/lib/c-api/wasmer.h
[c docs]: https://docs.rs/wasmer-c-api/*/wasmer_c_api/wasm_c_api/index.html

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

[swift integration]: https://github.com/AlwaysRightInstitute/SwiftyWasmer

[zig logo]: https://raw.githubusercontent.com/ziglang/logo/master/zig-favicon.png
[zig integration]: https://github.com/zigwasm/wasmer-zig

[dart logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/dart.svg
[dart integration]: https://github.com/dart-lang/wasm
[`wasm` pub package]: https://pub.dev/packages/wasm

[lisp integration]: https://github.com/helmutkian/cl-wasm-runtime

[OCaml logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/ocaml.svg
[OCaml integration]: https://github.com/wasmerio/wasmer-ocaml
[`wasmer` OCaml package]: https://opam.ocaml.org/packages/wasmer/

## 기여

도움을 주셔서 감사합니다! 💜

[Wasmer를 빌드](https://docs.wasmer.io/ecosystem/wasmer/building-from-source)하거나 [변경 사항을 테스트](https://docs.wasmer.io/ecosystem/wasmer/building-from-source/testing)하는 방법에 대한 문서를 확인하십시오.

## 커뮤니티
Wasmer에는 개발자의 기여가 있는 훌륭한 커뮤니티가 있습니다. 환영합니다! 꼭 참여해주세요! 👋

- [Wasmer Community Slack](https://slack.wasmer.io/)
- [Wasmer on Twitter](https://twitter.com/wasmerio)
- [Wasmer on Facebook](https://www.facebook.com/wasmerio)
- [Email](mailto:hello@wasmer.io)
