<div align="center">
  <a href="https://wasmer.io" target="_blank" rel="noopener noreferrer">
    <img width="300" src="https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/logo.png" alt="Wasmerロゴ">
  </a>
  
  <p>
    <a href="https://github.com/wasmerio/wasmer/actions?query=workflow%3Abuild">
      <img src="https://github.com/wasmerio/wasmer/workflows/build/badge.svg?style=flat-square" alt="ビルドステータス">
    </a>
    <a href="https://github.com/wasmerio/wasmer/blob/main/LICENSE">
      <img src="https://img.shields.io/github/license/wasmerio/wasmer.svg?style=flat-square" alt="ライセンス">
    </a>
    <a href="https://slack.wasmer.io">
      <img src="https://img.shields.io/static/v1?label=Slack&message=join%20chat&color=brighgreen&style=flat-square" alt="Slackチャンネル">
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

[Wasmer](https://wasmer.io/) は、[WebAssembly](https://webassembly.org/) をベースとした非常に軽量なコンテナを実現します。デスクトップからクラウドや IoT デバイス上まで、どんな環境でも実行でき、さらに[*任意のプログラミング言語*](#他の言語とのインテグレーション)に埋め込むこともできます。

> この readme は、次の言語でも利用可能です。[🇩🇪 Deutsch-ドイツ語](https://github.com/wasmerio/wasmer/blob/main/docs/de/README.md) • [🇨🇳 中文-Chinese](https://github.com/wasmerio/wasmer/blob/main/docs/cn/README.md) • [🇬🇧 English-英語](https://github.com/wasmerio/wasmer/blob/main/README.md) • [🇪🇸 Español-Spanish](https://github.com/wasmerio/wasmer/blob/main/docs/es/README.md) • [🇫🇷 Français-French](https://github.com/wasmerio/wasmer/blob/main/docs/fr/README.md)

## 機能

* **高速かつ安全**。WebAssembly を完全なサンドボックス環境内で*ネイティブに近い*スピードで実行します。

* **プラガブル**。異なるコンパイルフレームワーク (LLVM、Cranelift など...) をサポートしているため、ニーズに合った最適なフレームワークを選択できます。

* **ユニバーサル**。どんなプラットフォーム上 (macOS、Linux、Windows) でも、どんな*チップセット*上でも実行できます。

* **標準に準拠**。ランタイムは[公式の WebAssembly テストスイート](https://github.com/WebAssembly/testsuite)に通っており、[WASI](https://github.com/WebAssembly/WASI) と [Emscripten](https://emscripten.org/) をサポートします。

## クイックスタート

Wasmer は依存関係なしで動作します。以下のコマンドでインストーラーを使用してインストールできます。

```sh
curl https://get.wasmer.io -sSfL | sh
```

<details>
  <summary>PowerShell の場合 (Windows)</summary>
  <p>

```powershell
iwr https://win.wasmer.io -useb | iex
```

</p>
</details>

> Homebrew、Scoop、Cargo など、他のインストール方法については、[wasmer-install](https://github.com/wasmerio/wasmer-install) を参照してください。


#### WebAssembly ファイルの実行

Wasmer をインストールしたら、初めての WebAssembly ファイルの実行準備が完了です！ 🎉

QuickJS ([qjs.wasm](https://registry-cdn.wapm.io/contents/_/quickjs/0.0.3/build/qjs.wasm)) を実行することで、すぐに始められます。

```bash
$ wasmer qjs.wasm
QuickJS - Type "\h" for help
qjs >
```

#### 次にできること

- [Rust アプリケーションから Wasmer を使用する](https://docs.wasmer.io/integrations/rust)
- [WAPM で Wasm パッケージを公開する](https://docs.wasmer.io/ecosystem/wapm/publishing-your-package)
- [Wasmer についてさらに学ぶ](https://medium.com/wasmer/)

## 他の言語とのインテグレーション

📦 Wasmer ランタイムは**他の言語に組み込んで**使用できるため、WebAssembly は*どんな場所でも*利用できます。

| &nbsp; | Language | Package | Docs |
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
| ![R logo] | [**R**][R integration] | *公開パッケージなし* | [Docs][r docs] |
| ![Postgres logo] | [**Postgres**][Postgres integration] | *公開パッケージなし* | [Docs][postgres docs] |
|  | [**Swift**][Swift integration] | *公開パッケージなし* | |
| ![Zig logo] | [**Zig**][Zig integration] | *no published package* | |
| ![Ocaml logo] | [**OCaml**][OCaml integration] | [`wasmer` OCaml package] | |

[👋 言語が見当たらない？](https://github.com/wasmerio/wasmer/issues/new?assignees=&labels=%F0%9F%8E%89+enhancement&template=---feature-request.md&title=)

[rust logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/rust.svg
[rust integration]: https://github.com/wasmerio/wasmer/tree/main/lib/api
[`wasmer` rust crate]: https://crates.io/crates/wasmer/
[rust docs]: https://wasmerio.github.io/wasmer/crates/wasmer

[c logo]: https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/languages/c.svg
[c integration]: https://github.com/wasmerio/wasmer/tree/main/lib/c-api
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

## コントリビューション

**どんな形での貢献も歓迎です。コミュニティの新しいメンバーからの貢献は特に歓迎します。** 💜

Wasmer ランタイムのビルド方法は、[素晴らしいドキュメント](https://docs.wasmer.io/ecosystem/wasmer/building-from-source)で確認できます！

### テスト

テストを実行したいですか？ [Wasmer docs で方法を説明](https://docs.wasmer.io/ecosystem/wasmer/building-from-source/testing)しています。

## コミュニティ

Wasmer には、開発者とコントリビューターの素晴らしいコミュニティがあります。ようこそ！ あなたも是非参加してください！ 👋

### チャンネル

- [Slack](https://slack.wasmer.io/)
- [Twitter](https://twitter.com/wasmerio)
- [Facebook](https://www.facebook.com/wasmerio)
- [Email](mailto:hello@wasmer.io)
