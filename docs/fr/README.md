<div align="center">
  <a href="https://wasmer.io" target="_blank" rel="noopener noreferrer">
    <img width="300" src="https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/logo.png" alt="Logo Wasmer">
  </a>
  
  <p>
    <a href="https://github.com/wasmerio/wasmer/actions?query=workflow%3Abuild">
      <img src="https://github.com/wasmerio/wasmer/workflows/build/badge.svg?style=flat-square" alt="État des tests">
    </a>
    <a href="https://github.com/wasmerio/wasmer/blob/master/LICENSE">
      <img src="https://img.shields.io/github/license/wasmerio/wasmer.svg?style=flat-square" alt="Licence">
    </a>
    <a href="https://slack.wasmer.io">
      <img src="https://img.shields.io/static/v1?label=Slack&message=join%20chat&color=brighgreen&style=flat-square" alt="Salon Slack">
    </a> 
  </p>

  <h3>
    <a href="https://wasmer.io/">Web</a>
    <span> • </span>
    <a href="https://docs.wasmer.io">Documentation</a>
    <span> • </span>
    <a href="https://slack.wasmer.io/">Chat</a>
  </h3>

</div>

<br />

[Wasmer](https://wasmer.io/) permet l'utilisation de conteneurs super légers basés sur [WebAssembly](https://webassembly.org/) qui peuvent fonctionner n'importe où : du bureau au cloud en passant par les appareils IoT, et également intégrés dans [*une multitude de langages de programmation*](https://github.com/wasmerio/wasmer#language-integrations).

> This readme is also available in: [🇩🇪 Deutsch-Allemand](https://github.com/wasmerio/wasmer/blob/master/docs/de/README.md) • [🇬🇧 English-Anglaise](https://github.com/wasmerio/wasmer/blob/master/README.md) • [🇪🇸 Español-Espagnol](https://github.com/wasmerio/wasmer/blob/master/docs/es/README.md) • [🇨🇳 中文-Chinoise](https://github.com/wasmerio/wasmer/blob/master/docs/cn/README.md) • [🇯🇵 日本語-japonais](https://github.com/wasmerio/wasmer/blob/master/docs/ja/README.md)

## Fonctionnalités

* **Rapide et sûr**. Wasmer exécute WebAssembly à une vitesse *quasi native* dans un environnement entièrement contrôlé (bac à sable, _sandbox_).

* **Modulaire**. Wasmer prend en charge différents frameworks de compilation pour répondre au mieux à vos besoins (LLVM, Cranelift ...).

* **Universel**. Vous pouvez exécuter Wasmer sur n'importe quelle *plate-forme* (macOS, Linux et Windows) et *processeur*.

* **Conforme aux normes**. Wasmer passe [la suite de tests officielle de WebAssembly](https://github.com/WebAssembly/testsuite) prenant en charge [WASI](https://github.com/WebAssembly/WASI) et [Emscripten](https://emscripten.org/)

## Quickstart

Wasmer est livré sans aucune dépendance. Vous pouvez l'installer à l'aide des programmes d'installation ci-dessous :

```sh
curl https://get.wasmer.io -sSfL | sh
```

<details>
  <summary>Avec PowerShell (Windows)</summary>
  <p>

```powershell
iwr https://win.wasmer.io -useb | iex
```

</p>
</details>

> Voir [wasmer-install](https://github.com/wasmerio/wasmer-install) pour plus d'options d'installation: Homebrew, Scoop, Cargo...


#### Exécution d'un fichier WebAssembly

Après avoir installé Wasmer, vous devriez être prêt à exécuter votre premier fichier WebAssemby ! 🎉

Vous pouvez commencer par exécuter QuickJS : [qjs.wasm](https://registry-cdn.wapm.io/contents/_/quickjs/0.0.3/build/qjs.wasm)

```bash
$ wasmer qjs.wasm
QuickJS - Type "\h" for help
qjs >
```

#### Voici ce que vous pouvez faire ensuite

- [Utilisez Wasmer depuis votre application Rust](https://docs.wasmer.io/integrations/rust)
- [Publier un paquet Wasm sur WAPM](https://docs.wasmer.io/ecosystem/wapm/publishing-your-package)
- [En savoir plus sur Wasmer](https://medium.com/wasmer/)

## Intégrations

📦  Wasmer peut être utilisé comme une bibliothèque **intégrée dans différents langages**, vous pouvez donc utiliser WebAssembly _n'import où_.

| &nbsp; | Langage de programmation | Package | Docs |
|-|-|-|-|
| ![Rust logo] | [**Rust**][Rust integration] | [`wasmer` Rust crate] | [Docs][rust docs]
| ![C logo] | [**C/C++**][C integration] | [`wasmer.h` headers] | [Docs][c docs] |
| ![C# logo] | [**C#**][C# integration] | [`WasmerSharp` NuGet package] | [Docs][c# docs] |
| ![D logo] | [**D**][D integration] | [`wasmer` Dub package] | [Docs][d docs] |
| ![Python logo] | [**Python**][Python integration] | [`wasmer` PyPI package] | [Docs][python docs] |
| ![JS logo] | [**Javascript**][JS integration] | [`@wasmerio` NPM packages] | [Docs][js docs] |
| ![Go logo] | [**Go**][Go integration] | [`wasmer` Go package] | [Docs][go docs] |
| ![PHP logo] | [**PHP**][PHP integration] | [`wasm` PECL package] | [Docs][php docs] |
| ![Ruby logo] | [**Ruby**][Ruby integration] | [`wasmer` Ruby Gem] | [Docs][ruby docs] |
| ![Java logo] | [**Java**][Java integration] | [`wasmer/wasmer-jni` Bintray package] | [Docs][java docs] |
| ![Elixir logo] | [**Elixir**][Elixir integration] | [`wasmex` hex package] | [Docs][elixir docs] |
| ![R logo] | [**R**][R integration] | *no published package* | [Docs][r docs] |
| ![Postgres logo] | [**Postgres**][Postgres integration] | *no published package* | [Docs][postgres docs] |
|  | [**Swift**][Swift integration] | *no published package* | |
| ![Zig logo] | [**Zig**][Zig integration] | *no published package* | |
| ![Ocaml logo] | [**OCaml**][OCaml integration] | [`wasmer` OCaml package] | |

[👋  Il manque un langage ?](https://github.com/wasmerio/wasmer/issues/new?assignees=&labels=%F0%9F%8E%89+enhancement&template=---feature-request.md&title=)

[rust logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/rust.svg
[rust integration]: https://github.com/wasmerio/wasmer/tree/master/lib/api
[`wasmer` rust crate]: https://crates.io/crates/wasmer/
[rust docs]: https://wasmerio.github.io/wasmer/crates/wasmer

[c logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/c.svg
[c integration]: https://github.com/wasmerio/wasmer/tree/master/lib/c-api
[`wasmer.h` headers]: https://wasmerio.github.io/wasmer/c/
[c docs]: https://wasmerio.github.io/wasmer/c/

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

## Contribuer

**Nous accueillons toutes formes de contributions, en particulier de la part des nouveaux membres de notre communauté**. 💜

Vous pouvez vérifier comment compiler Wasmer dans [notre documentation](https://docs.wasmer.io/ecosystem/wasmer/building-from-source)!

### Test

Vous voulez des tests ? La [documentation de Wasmer](https://docs.wasmer.io/ecosystem/wasmer/building-from-source/testing) vous montrera comment les exécuter.

## Communauté

Wasmer a une incroyable communauté de développeurs et de contributeurs. Bienvenue et rejoignez-nous ! 👋

### Canaux de communications

- [Slack](https://slack.wasmer.io/)
- [Twitter](https://twitter.com/wasmerio)
- [Facebook](https://www.facebook.com/wasmerio)
- [Email](mailto:hello@wasmer.io)
