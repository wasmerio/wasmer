<div align="center">
  <a href="https://wasmer.io" target="_blank" rel="noopener noreferrer">
    <img width="300" src="https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/logo.png" alt="Wasmer Logo">
  </a>

  <p>
    <a href="https://github.com/wasmerio/wasmer/actions?query=workflow%3Abuild">
      <img src="https://github.com/wasmerio/wasmer/workflows/build/badge.svg?style=flat-square" alt="Build Status">
    </a>
    <a href="https://github.com/wasmerio/wasmer/blob/master/LICENSE">
      <img src="https://img.shields.io/github/license/wasmerio/wasmer.svg" alt="Lizenz">
    </a>
    <a href="https://docs.wasmer.io">
      <img src="https://img.shields.io/static/v1?label=Docs&message=docs.wasmer.io&color=blue" alt="Wasmer Doku">
    </a>
    <a href="https://slack.wasmer.io">
      <img src="https://img.shields.io/static/v1?label=Slack&message=teilnehmen&color=brighgreen" alt="Slack Kanal">
    </a>
  </p>
</div>

<br />

Wasmer ist eine _schnelle_ und _sichere_ [**WebAssembly**](https://webassembly.org) Runtime, die das Ausführen von
_schlanken Containern_ überall ermöglicht: auf dem *Desktop* in der *Cloud*, so wie auf *Edge* und *IoT* Geräten.

> _Die README ist auch in folgenden Sprachen verfügbar:
[🇨🇳 中文-Chinesisch](https://github.com/wasmerio/wasmer/blob/master/docs/cn/README.md) • 
[🇬🇧 English-Englisch](https://github.com/wasmerio/wasmer/blob/master/README.md) •
[🇪🇸 Español-Spanisch](https://github.com/wasmerio/wasmer/blob/master/docs/es/README.md) • 
[🇫🇷 Français-Französisch](https://github.com/wasmerio/wasmer/blob/master/docs/fr/README.md) • 
[🇯🇵 日本語-Japanisch](https://github.com/wasmerio/wasmer/blob/master/docs/ja/README.md)_.

### Leistungsmerkmale

* Standardmäßig sicher. Kein Datei-, Netzwerk- oder Umgebungszugriff, sofern nicht explizit aktiviert.
* Unterstützt [WASI](https://github.com/WebAssembly/WASI) und [Emscripten](https://emscripten.org/) standardmäßig.
* Schnell. Führt WebAssembly in nahezu nativer Geschwindigkeit aus.
* Einbettbar in [mehrere Programmiersprachen](https://github.com/wasmerio/wasmer/#-language-integrations)
* Kompatibel mit den neuesten Empfehlungen für WebAssembly (SIMD, Referenztypen, Threads, ...)

### Installation

Wasmer CLI wird als eine einzige ausführbare Datei ohne Abhängigkeiten ausgeliefert.

```sh
curl https://get.wasmer.io -sSfL | sh
```


<details>
  <summary>Weitere Installationsmöglichkeiten (Powershell, Brew, Cargo, ...)</summary>
  
  _Wasmer kann über verschiedene Paketmanager installiert werden. Wählen Sie den für Ihre Umgebung am besten geeigneten aus:_
  
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

    _Note: All the available
    features are described in the [`wasmer-cli`
    crate docs](https://github.com/wasmerio/wasmer/tree/master/lib/cli/README.md)_

    ```sh
    cargo install wasmer-cli
    ```

  > Suchen Sie nach weiteren Installationsmöglichkeiten? Im [`wasmer-install`
  Repository](https://github.com/wasmerio/wasmer-install) können Si mehr erfahren!
</details>

### Schnellstart

Sie können beginnen,
[QuickJS](https://github.com/bellard/quickjs/) auszuführen, eine kleine und
einbettbare Javascript Engine, die als WebAssembly Modul kompiliert ist: ([`qjs.wasm`](https://registry-cdn.wapm.io/contents/_/quickjs/0.0.3/build/qjs.wasm)):

```bash
$ wasmer qjs.wasm
QuickJS - Type "\h" for help
qjs > const i = 1 + 2;
qjs > console.log("hello " + i);
hello 3
```

#### Folgendes können Sie als nächstes tun:

- [Wasmer für eine Rust Anwendung nutzen](https://docs.wasmer.io/integrations/rust)
- [Ein asm Paket auf WAPM veröffentlichen](https://docs.wasmer.io/ecosystem/wapm/publishing-your-package)
- [Mehr zu Wasmer lesen](https://medium.com/wasmer/)

## 📦 Unterstützte Sprachen

Die Wasmer-Laufzeit kann als Bibliothek **eingebettet in verschiedenen
Sprachen** verwendet werden, so dass Sie WebAssembly _überall_ einsetzen können.

| | Sprache | Paket | Dokumentation |
|-|-|-|-|
| ![Rust logo] | [**Rust**][Rust Integration] | [`wasmer` Rust crate] | [Lernen][rust docs]
| ![C logo] | [**C/C++**][C Integration] | [`wasmer.h` header] | [Lernen][c docs] |
| ![C# logo] | [**C#**][C# Integration] | [`WasmerSharp` NuGet Paket] | [Lernen][c# docs] |
| ![D logo] | [**D**][D Integration] | [`wasmer` Dub Paket] | [Lernen][d docs] |
| ![Python logo] | [**Python**][Python Integration] | [`wasmer` PyPI Paket] | [Lernen][python docs] |
| ![JS logo] | [**Javascript**][JS Integration] | [`@wasmerio` NPM Paket] | [Lernen][js docs] |
| ![Go logo] | [**Go**][Go Integration] | [`wasmer` Go Paket] | [Lernen][go docs] |
| ![PHP logo] | [**PHP**][PHP Integration] | [`wasm` PECL Paket] | [Lernen][php docs] |
| ![Ruby logo] | [**Ruby**][Ruby Integration] | [`wasmer` Ruby Gem] | [Lernen][ruby docs] |
| ![Java logo] | [**Java**][Java Integration] | [`wasmer/wasmer-jni` Bintray Paket] | [Lernen][java docs] |
| ![Elixir logo] | [**Elixir**][Elixir Integration] | [`wasmex` hex Paket] | [Lernen][elixir docs] |
| ![R logo] | [**R**][R Integration] | *kein Paket veröffentlicht* | [Lernen][r docs] |
| ![Postgres logo] | [**Postgres**][Postgres Integration] | *kein Paket veröffentlicht* | [Lernen][postgres docs] |
|  | [**Swift**][Swift Integration] | *kein Paket veröffentlicht* | |
| ![Zig logo] | [**Zig**][Zig Integration] | *kein Paket veröffentlicht* | |
| ![Dart logo] | [**Dart**][Dart Integration] | [`wasm` pub Paket] | |
| ![Ocaml logo] | [**OCaml**][OCaml integration] | [`wasmer` OCaml package] | |

[👋&nbsp;&nbsp;Fehlt eine Sprache?](https://github.com/wasmerio/wasmer/issues/new?assignees=&labels=%F0%9F%8E%89+enhancement&template=---feature-request.md&title=)

[rust logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/rust.svg
[rust integration]: https://github.com/wasmerio/wasmer/tree/master/lib/api
[`wasmer` rust crate]: https://crates.io/crates/wasmer/
[rust docs]: https://wasmerio.github.io/wasmer/crates/wasmer

[c logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/c.svg
[c integration]: https://github.com/wasmerio/wasmer/tree/master/lib/c-api
[`wasmer.h` header]: https://github.com/wasmerio/wasmer/blob/master/lib/c-api/wasmer.h
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

[dart logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/dart.svg
[dart integration]: https://github.com/dart-lang/wasm
[`wasm` pub package]: https://pub.dev/packages/wasm

[OCaml logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/ocaml.svg
[OCaml integration]: https://github.com/wasmerio/wasmer-ocaml
[`wasmer` OCaml package]: https://opam.ocaml.org/packages/wasmer/

## Unterstützen

Wir sind dankbar für Ihre Hilfe! 💜

Lesen Sie in unserer Dokumentation nach, wie man [Wasmer aus dem
Quellcode kompiliert](https://docs.wasmer.io/ecosystem/wasmer/building-from-source) oder [testen Sie Änderungen](https://docs.wasmer.io/ecosystem/wasmer/building-from-source/testing).

## Community

Wasmer hat eine wunderbare Community von Entwicklern und Mitwirkenden. Sie sind herzlich willkommen, bitte machen Sie mit! 👋

- [Wasmer Community auf Slack](https://slack.wasmer.io/)
- [Wasmer auf Twitter](https://twitter.com/wasmerio)
- [Wasmer auf Facebook](https://www.facebook.com/wasmerio)
- [Email](mailto:hello@wasmer.io)
