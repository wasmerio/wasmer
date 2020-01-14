<p align="center">
  <a href="https://wasmer.io" target="_blank" rel="noopener noreferrer">
    <img width="300" src="https://raw.githubusercontent.com/wasmerio/wasmer/master/logo.png" alt="Wasmer logo">
  </a>
</p>

<p align="center">
  <a href="https://dev.azure.com/wasmerio/wasmer/_build/latest?definitionId=3&branchName=master">
    <img src="https://img.shields.io/azure-devops/build/wasmerio/wasmer/3.svg?style=flat-square" alt="Build Status">
  </a>
  <a href="https://docs.wasmer.io">
    <img src="https://img.shields.io/badge/Docs-docs.wasmer.io-blue?style=flat-square" alt="Documentation">
  </a>
  <a href="https://github.com/wasmerio/wasmer/blob/master/LICENSE">
    <img src="https://img.shields.io/github/license/wasmerio/wasmer.svg?style=flat-square" alt="License">
  </a>
  <a href="https://spectrum.chat/wasmer">
    <img src="https://withspectrum.github.io/badge/badge.svg" alt="Join the Wasmer Community">
  </a>
  <a href="https://twitter.com/wasmerio">
    <img alt="Follow @wasmerio on Twitter" src="https://img.shields.io/twitter/follow/wasmerio?label=%40wasmerio&style=flat-square">
  </a>
</p>

## Introduction

[Wasmer](https://wasmer.io/) is a standalone WebAssembly runtime for running WebAssembly [outside of the browser](https://webassembly.org/docs/non-web/), supporting [WASI](https://github.com/WebAssembly/WASI) and [Emscripten](https://emscripten.org/). Wasmer can be used standalone (via the CLI) and embedded in different languages, running in x86 and [ARM devices](https://medium.com/wasmer/running-webassembly-on-arm-7d365ed0e50c).

Install the Wasmer and [WAPM](https://wapm.io) cli with:

```sh
curl https://get.wasmer.io -sSfL | sh
```

> Note: *Wasmer is also [available on Windows](https://github.com/wasmerio/wasmer/releases)*

### Languages

Wasmer runtime can be used as a library embedded in different languages, so you can use WebAssembly anywhere:

| &nbsp; | Language | Author(s) | Maintenance | Release | Stars |
|-|-|-|-|-|-|
| ![Rust logo](./docs/assets/languages/rust.svg) | [**Rust**](https://github.com/wasmerio/wasmer-rust-example) | Wasmer | actively developed | <a href="https://crates.io/crates/wasmer-runtime/" target="_blank">![last release](https://img.shields.io/crates/v/wasmer-runtime?style=flat-square)</a> | ![number of Github stars](https://img.shields.io/github/stars/wasmerio/wasmer?style=flat-square) |
| ![C logo](./docs/assets/languages/c.svg) | [**C/C++**](https://github.com/wasmerio/wasmer-c-api) | Wasmer | actively developed | <a href="https://github.com/wasmerio/wasmer-c-api/" target="_blank">![last release](https://img.shields.io/github/v/release/wasmerio/wasmer?style=flat-square)</a> | ![number of Github stars](https://img.shields.io/github/stars/wasmerio/wasmer?style=flat-square) |
| ![Python logo](./docs/assets/languages/python.svg) | [**Python**](https://github.com/wasmerio/python-ext-wasm) | Wasmer | actively developed | <a href="https://pypi.org/project/wasmer/" target="_blank">![last release](https://img.shields.io/pypi/v/wasmer?style=flat-square)</a> | ![number of Github stars](https://img.shields.io/github/stars/wasmerio/python-ext-wasm?style=flat-square) |
| ![Go logo](./docs/assets/languages/go.svg) | [**Go**](https://github.com/wasmerio/go-ext-wasm) | Wasmer | actively developed | <a href="https://github.com/wasmerio/go-ext-wasm" target="_blank">![last release](https://img.shields.io/github/v/release/wasmerio/go-ext-wasm?style=flat-square)</a> | ![number of Github stars](https://img.shields.io/github/stars/wasmerio/go-ext-wasm?style=flat-square) |
| ![PHP logo](./docs/assets/languages/php.svg) | [**PHP**](https://github.com/wasmerio/php-ext-wasm) | Wasmer | actively developed | <a href="https://pecl.php.net/package/wasm" target="_blank">![last release](https://img.shields.io/github/v/release/wasmerio/php-ext-wasm?style=flat-square)</a> | ![number of Github stars](https://img.shields.io/github/stars/wasmerio/php-ext-wasm?style=flat-square) |
| ![Ruby logo](./docs/assets/languages/ruby.svg) | [**Ruby**](https://github.com/wasmerio/ruby-ext-wasm) | Wasmer | actively developed | <a href="https://rubygems.org/gems/wasmer" target="_blank">![last release](https://img.shields.io/gem/v/wasmer?style=flat-square)</a> | ![number of Github stars](https://img.shields.io/github/stars/wasmerio/ruby-ext-wasm?style=flat-square) |
| ![Postgres logo](./docs/assets/languages/postgres.svg) | [**Postgres**](https://github.com/wasmerio/postgres-ext-wasm) | Wasmer | actively developed | <a href="https://github.com/wasmerio/postgres-ext-wasm" target="_blank">![last release](https://img.shields.io/github/v/release/wasmerio/postgres-ext-wasm?style=flat-square)</a> | ![number of Github stars](https://img.shields.io/github/stars/wasmerio/postgres-ext-wasm?style=flat-square) |
| ![JS Logo](./docs/assets/languages/js.svg) | [**JavaScript**](https://github.com/wasmerio/wasmer-js) | Wasmer | actively developed | <a href="https://www.npmjs.com/package/@wasmer/wasi" target="_blank">![last release](https://img.shields.io/npm/v/@wasmer/wasi?style=flat-square)</a> | ![number of Github stars](https://img.shields.io/github/stars/wasmerio/wasmer-js?style=flat-square) |
| ![C# logo](./docs/assets/languages/csharp.svg) | [**C#/.Net**](https://github.com/migueldeicaza/WasmerSharp) | [Miguel de Icaza](https://github.com/migueldeicaza) | actively developed | <a href="https://www.nuget.org/packages/WasmerSharp/" target="_blank">![last release](https://img.shields.io/nuget/v/WasmerSharp?style=flat-square)</a> | ![number of Github stars](https://img.shields.io/github/stars/migueldeicaza/WasmerSharp?style=flat-square) |
| ![R logo](./docs/assets/languages/r.svg) | [**R**](https://github.com/dirkschumacher/wasmr) | [Dirk Schumacher](https://github.com/dirkschumacher) | actively developed | | ![number of Github stars](https://img.shields.io/github/stars/dirkschumacher/wasmr?style=flat-square) |
| ![Swift logo](./docs/assets/languages/swift.svg) | [**Swift**](https://github.com/markmals/swift-ext-wasm) | [Mark Malström](https://github.com/markmals/) | passively maintained | | ![number of Github stars](https://img.shields.io/github/stars/markmals/swift-ext-wasm?style=flat-square) |
| ❓ | [your language is missing?](https://github.com/wasmerio/wasmer/issues/new?assignees=&labels=%F0%9F%8E%89+enhancement&template=---feature-request.md&title=) | | | |

### Usage

Wasmer can execute both the standard binary format (`.wasm`) and the text
format defined by the WebAssembly reference interpreter (`.wat`).

Once installed, you will be able to run any WebAssembly files (_including Lua, PHP, SQLite and nginx!_):

```sh
# Run Lua
wasmer examples/lua.wasm
```

*You can find more `wasm/wat` examples in the [examples](./examples) directory.*

### Docs

Wasmer documentation lives on [docs.wasmer.io](https://docs.wasmer.io).


## Code Structure

Wasmer is structured into different directories:

- [`src`](./src): code related to the Wasmer executable itself
- [`lib`](./lib): modularized libraries that Wasmer uses under the hood
- [`examples`](./examples): some useful examples for getting started with Wasmer

## Dependencies

Building Wasmer requires [rustup](https://rustup.rs/).

To build Wasmer on Windows, download and run [`rustup-init.exe`](https://win.rustup.rs/)
then follow the onscreen instructions.

To build on other systems, run:

```sh
curl https://sh.rustup.rs -sSf | sh
```

### Other dependencies

Please select your operating system:

<details>
  <summary><b>macOS</b></summary>
  <p>

#### macOS

If you have [Homebrew](https://brew.sh/) installed:

```sh
brew install cmake
```

Or, if you have [MacPorts](https://www.macports.org/install.php):

```sh
sudo port install cmake
```

  </p>
</details>

<details>
  <summary><b>Debian-based Linuxes</b></summary>
  <p>

#### Debian-based Linuxes

```sh
sudo apt install cmake pkg-config libssl-dev
```

  </p>
</details>

<details>
  <summary><b>FreeBSD</b></summary>
  <p>

#### FreeBSD

```sh
pkg install cmake
```

  </p>
</details>

<details>
  <summary><b>Windows</b></summary>
  <p>

#### Windows (MSVC)

Windows support is _experimental_. WASI is fully supported, but Emscripten support is in the works (this means
nginx and Lua do not work on Windows - you can track the progress on [this issue](https://github.com/wasmerio/wasmer/issues/176)).

1. Install [Visual Studio](https://visualstudio.microsoft.com/thank-you-downloading-visual-studio/?sku=Community&rel=15)

2. Install [Rust for Windows](https://win.rustup.rs)

3. Install [Git for Windows](https://git-scm.com/download/win). Allow it to add `git.exe` to your PATH (default
   settings for the installer are fine).

4. Install [CMake](https://cmake.org/download/). Ensure CMake is in your PATH.

5. Install [LLVM 8.0](https://prereleases.llvm.org/win-snapshots/LLVM-8.0.0-r351033-win64.exe)
     </p>
   </details>

## Building

[![Rustc Version 1.39+](https://img.shields.io/badge/rustc-1.39+-red.svg?style=flat-square)](https://blog.rust-lang.org/2019/11/07/Rust-1.39.0.html)

Wasmer is built with [Cargo](https://crates.io/), the Rust package manager.

The Singlepass backend requires nightly, so if you want to use it,

Set Rust Nightly:

```
rustup default nightly
```

Otherwise an up to date (see badge above) version of stable Rust will work.

And install Wasmer

```sh
# checkout code
git clone https://github.com/wasmerio/wasmer.git
cd wasmer

# install tools
make release-clif # To build with cranelift (default)

make release-llvm # To build with llvm support

make release-singlepass # To build with singlepass support

# or
make release # To build with singlepass, cranelift and llvm support
```

## Testing

Thanks to [spec tests](https://github.com/wasmerio/wasmer/tree/master/lib/spectests/spectests) we can ensure 100% compatibility with the WebAssembly spec test suite.

You can run all the tests with:

```sh
rustup default nightly
make test
```

### Testing backends

Each backend can be tested separately:

* Singlepass: `make singlepass`
* Cranelift: `make cranelift`
* LLVM: `make llvm`

### Testing integrations

Each integration can be tested separately:

* Spec tests: `make spectests`
* Emscripten: `make emtests`
* WASI: `make wasitests`
* Middleware: `make middleware`
* C API: `make capi`

## Benchmarking

Benchmarks can be run with:

```sh
make bench-[backend]

# for example
make bench-singlepass
```

## Roadmap

Wasmer is an open project guided by strong principles, aiming to be modular, flexible and fast. It is open to the community to help set its direction.

Below are some of the goals of this project (in order of priority):

- [x] It should be 100% compatible with the [WebAssembly spec tests](https://github.com/wasmerio/wasmer/tree/master/lib/spectests/spectests)
- [x] It should be fast _(partially achieved)_
- [x] Support WASI - released in [0.3.0](https://github.com/wasmerio/wasmer/releases/tag/0.3.0)
- [x] Support Emscripten calls
- [ ] Support Go JS ABI calls _(in the works)_

## Architecture

If you would like to know how Wasmer works under the hood, please see [docs/architecture.md](./docs/architecture.md).

## License

Wasmer is primarily distributed under the terms of the [MIT license](http://opensource.org/licenses/MIT) ([LICENSE](./LICENSE)).

[ATTRIBUTIONS](./ATTRIBUTIONS.md)
