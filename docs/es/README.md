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
    <a href="https://wasmer.io/">Web</a>
    <span> • </span>
    <a href="https://docs.wasmer.io">Documentación</a>
    <span> • </span>
    <a href="https://slack.wasmer.io/">Chat</a>
  </h3>

</div>

<br />

[Wasmer](https://wasmer.io/) hace posible tener contenedores ultraligeros basados en [WebAssembly](https://webassembly.org/) que pueden ser ejecutados en cualquier sitio: desde tu ordenador hasta la nube y dispositivos de IoT, además de poder ser ejecutados [*en cualquier lenguaje de programación*](https://github.com/wasmerio/wasmer#language-integrations).

> This README is also available in: [🇩🇪 Deutsch-Alemán](https://github.com/wasmerio/wasmer/blob/master/docs/de/README.md) • [🇬🇧 English-Inglés](https://github.com/wasmerio/wasmer/blob/master/README.md) • [🇫🇷 Français-Francés](https://github.com/wasmerio/wasmer/blob/master/docs/fr/README.md) • [🇨🇳 中文-Chino](https://github.com/wasmerio/wasmer/blob/master/docs/cn/README.md) • [🇯🇵 日本語-japonés](https://github.com/wasmerio/wasmer/blob/master/docs/ja/README.md).

## Funcionalidades

* **Rápido y Seguro**. Wasmer ejecuta WebAssembly a velocidades *nativas* en un entorno completamente protegido.

* **Extendible**. Wasmer soporta diferentes métodos de compilación dependiendo de tus necesidades (LLVM, Cranelift...).

* **Universal**. Puedes ejecutar Wasmer en cualquier *platforma* (macOS, Linux y Windows) y *chip*.

* **Respeta los estándares**. Wasmer pasa los [tests oficiales de WebAssembly](https://github.com/WebAssembly/testsuite) siendo compatible con [WASI](https://github.com/WebAssembly/WASI) y [Emscripten](https://emscripten.org/).

## Empezamos?

Wasmer no requiere ninguna dependencia. Puedes instalarlo con uno de estos instaladores:

```sh
curl https://get.wasmer.io -sSfL | sh
```

<details>
  <summary>Con PowerShell (Windows)</summary>
  <p>

```powershell
iwr https://win.wasmer.io -useb | iex
```

</p>
</details>

> Visita [wasmer-install](https://github.com/wasmerio/wasmer-install) para más opciones de instalación: Homebrew, Scoop, Cargo...


#### Ejecuta un archivo WebAssembly

¡Después de instalar Wasmer deberías estar listo para ejecutar tu primer módulo de WebAssembly! 🎉

Puedes empezar corriendo QuickJS: [qjs.wasm](https://registry-cdn.wapm.io/contents/_/quickjs/0.0.3/build/qjs.wasm)

```bash
$ wasmer qjs.wasm
QuickJS - Type "\h" for help
qjs >
```

#### Esto es lo que puedes hacer:

- [Usa Wasmer desde tu aplicación de Rust](https://docs.wasmer.io/integrations/rust)
- [Publica un paquete de Wasm en WAPM](https://docs.wasmer.io/ecosystem/wapm/publishing-your-package)
- [Lee más sobre Wasmer](https://medium.com/wasmer/)

## Integraciones en diferentes Lenguajes

📦 Wasmer puede ser usado como una librería **integrada en diferentes lenguajes de programación**, para que puedas ejecutar WebAssembly _en cualquier sitio_.

| &nbsp; | Lenguaje | Librería | Documentación |
|-|-|-|-|
| ![Rust logo] | [**Rust**][Rust integration] | [`wasmer` en crates.io] | [Documentación][rust docs]
| ![C logo] | [**C/C++**][C integration] | [cabecera `wasmer.h`] | [Documentación][c docs] |
| ![C# logo] | [**C#**][C# integration] | [`WasmerSharp` en NuGet] | [Documentación][c# docs] |
| ![D logo] | [**D**][D integration] | [`wasmer` en Dug] | [Documentación][d docs] |
| ![Python logo] | [**Python**][Python integration] | [`wasmer` en PyPI] | [Documentación][python docs] |
| ![JS logo] | [**Javascript**][JS integration] | [`@wasmerio` en NPM] | [Documentación][js docs] |
| ![Go logo] | [**Go**][Go integration] | [`wasmer` en Go] | [Documentación][go docs] |
| ![PHP logo] | [**PHP**][PHP integration] | [`wasm` en PECL] | [Documentación][php docs] |
| ![Ruby logo] | [**Ruby**][Ruby integration] | [`wasmer` en Ruby Gems] | [Documentación][ruby docs] |
| ![Java logo] | [**Java**][Java integration] | [`wasmer/wasmer-jni` en Bintray] | [Documentación][java docs] |
| ![Elixir logo] | [**Elixir**][Elixir integration] | [`wasmex` en hex] | [Documentación][elixir docs] |
| ![R logo] | [**R**][R integration] | *sin paquete publicado* | [Documentación][r docs] |
| ![Postgres logo] | [**Postgres**][Postgres integration] | *sin paquete publicado* | [Documentación][postgres docs] |
|  | [**Swift**][Swift integration] | *sin paquete publicado* | |
| ![Zig logo] | [**Zig**][Zig integration] | *no published package* | |
| ![Ocaml logo] | [**OCaml**][OCaml integration] | [`wasmer` OCaml package] | |

[👋 Falta algún lenguaje?](https://github.com/wasmerio/wasmer/issues/new?assignees=&labels=%F0%9F%8E%89+enhancement&template=---feature-request.md&title=)

[rust logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/rust.svg
[rust integration]: https://github.com/wasmerio/wasmer/tree/master/lib/api
[`wasmer` en crates.io]: https://crates.io/crates/wasmer/
[rust docs]: https://wasmerio.github.io/wasmer/crates/wasmer

[c logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/c.svg
[c integration]: https://github.com/wasmerio/wasmer/tree/master/lib/c-api
[cabecera `wasmer.h`]: https://wasmerio.github.io/wasmer/c/
[c docs]: https://wasmerio.github.io/wasmer/c/

[c# logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/csharp.svg
[c# integration]: https://github.com/migueldeicaza/WasmerSharp
[`wasmersharp` en NuGet]: https://www.nuget.org/packages/WasmerSharp/
[c# docs]: https://migueldeicaza.github.io/WasmerSharp/

[d logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/d.svg
[d integration]: https://github.com/chances/wasmer-d
[`wasmer` en Dub]: https://code.dlang.org/packages/wasmer
[d docs]: https://chances.github.io/wasmer-d

[python logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/python.svg
[python integration]: https://github.com/wasmerio/wasmer-python
[`wasmer` en pypi]: https://pypi.org/project/wasmer/
[python docs]: https://github.com/wasmerio/wasmer-python#api-of-the-wasmer-extensionmodule

[go logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/go.svg
[go integration]: https://github.com/wasmerio/wasmer-go
[`wasmer` en go]: https://pkg.go.dev/github.com/wasmerio/wasmer-go/wasmer
[go docs]: https://pkg.go.dev/github.com/wasmerio/wasmer-go/wasmer?tab=doc

[php logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/php.svg
[php integration]: https://github.com/wasmerio/wasmer-php
[php docs]: https://wasmerio.github.io/wasmer-php/wasm/
[`wasm` en pecl]: https://pecl.php.net/package/wasm

[js logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/js.svg
[js integration]: https://github.com/wasmerio/wasmer-js
[`@wasmerio` en npm]: https://www.npmjs.com/org/wasmer
[js docs]: https://docs.wasmer.io/integrations/js/reference-api

[ruby logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/ruby.svg
[ruby integration]: https://github.com/wasmerio/wasmer-ruby
[`wasmer` en ruby gems]: https://rubygems.org/gems/wasmer
[ruby docs]: https://www.rubydoc.info/gems/wasmer/

[java logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/java.svg
[java integration]: https://github.com/wasmerio/wasmer-java
[`wasmer/wasmer-jni` en bintray]: https://bintray.com/wasmer/wasmer-jni/wasmer-jni
[java docs]: https://github.com/wasmerio/wasmer-java/#api-of-the-wasmer-library

[elixir logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/elixir.svg
[elixir integration]: https://github.com/tessi/wasmex
[elixir docs]: https://hexdocs.pm/wasmex/api-reference.html
[`wasmex` en hex]: https://hex.pm/packages/wasmex

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

## Contribuye

**Damos la bienvenida a cualquier forma de contribución, especialmente a los nuevos miembros de la comunidad** 💜

¡Puedes ver cómo crear el binario de Wasmer con [nuestros increíbles documentos](https://docs.wasmer.io/ecosystem/wasmer/building-from-source)!

### Tests

¿Quieres testear? Los [documentos de Wasmer te enseñarán cómo](https://docs.wasmer.io/ecosystem/wasmer/building-from-source/testing).

## Comunidad

Wasmer tiene una comunidad increíble de desarrolladores y colaboradores ¡Bienvenido, únete a nosotros! 👋

### Medios

- [Slack](https://slack.wasmer.io/)
- [Twitter](https://twitter.com/wasmerio)
- [Facebook](https://www.facebook.com/wasmerio)
- [Email](mailto:hello@wasmer.io)
