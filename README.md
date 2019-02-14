<p align="center"><a href="https://wasmer.io" target="_blank" rel="noopener noreferrer"><img width="400" src="https://raw.githubusercontent.com/wasmerio/wasmer/master/logo.png" alt="Wasmer logo"></a></p>

<p align="center">
  <a href="https://circleci.com/gh/wasmerio/wasmer/"><img src="https://img.shields.io/circleci/project/github/wasmerio/wasmer/master.svg" alt="Build Status"></a>
  <a href="https://github.com/wasmerio/wasmer/blob/master/LICENSE"><img src="https://img.shields.io/github/license/wasmerio/wasmer.svg" alt="License"></a>
  <a href="https://spectrum.chat/wasmer">
    <img alt="Join the Wasmer Community" src="https://withspectrum.github.io/badge/badge.svg" />
  </a>
</p>

## Introduction

[Wasmer](https://wasmer.io/) is a Standalone JIT WebAssembly runtime, aiming to be fully compatible with Emscripten, Rust and Go.

Install Wasmer with:

```sh
curl https://get.wasmer.io -sSfL | sh
```

_**NEW ✨**: Now you can also embed Wasmer in your Rust application, check our [example repo](https://github.com/wasmerio/wasmer-rust-example) to see how to do it!_

### Usage

`wasmer` can execute both the standard binary format (`.wasm`) and the text
format defined by the WebAssembly reference interpreter (`.wat`).

Once installed, you will be able to run any WebAssembly files (_including Nginx, and Lua!_):

```sh
# Run Lua
wasmer run examples/lua.wasm

# Run Nginx
wasmer run examples/nginx/nginx.wasm -- -p examples/nginx -c nginx.conf
```

## Code Structure

Wasmer is structured into different directories:

- [`src`](./src): code related to the wasmer excutable binary itself
- [`lib`](./lib): modularized libraries that Wasmer uses under the hood
- [`examples`](./examples): some useful examples to getting started with wasmer

## Dependencies

Building wasmer requires [rustup](https://rustup.rs/).

To build on Windows, download and run [`rustup-init.exe`](https://win.rustup.rs/)
then follow the onscreen instructions.

To build on other systems, run:

```sh
curl https://sh.rustup.rs -sSf | sh
```

### Other dependencies

Please select your operating system:

- [macOS](#macos)
- [Debian-based Linuxes](#debian-based-linuxes)
- [Microsoft Windows](#windows-msvc)

#### macOS

If you have [homebrew](https://brew.sh/) installed:

```sh
brew install cmake
```

Or, in case you have [ports](https://www.macports.org/install.php):

```sh
sudo port install cmake
```

#### Debian-based Linuxes

```sh
sudo apt install cmake
```

#### Windows (MSVC)

Windows support is _highly experimental_. Only simple wasm programs may be run, and no syscalls are allowed. This means
nginx and lua do not work on Windows. See [this issue for ongoing Emscripten syscall polyfills for Windows](https://github.com/wasmerio/wasmer/pull/176).

1. Install Python for Windows (https://www.python.org/downloads/release/python-2714/). The Windows x86-64 MSI installer is fine.
   You should change the installation to install the "Add python.exe to Path" feature.

2. Install Git for Windows (https://git-scm.com/download/win). DO allow it to add git.exe to the PATH (default
   settings for the installer are fine).

3. Install CMake (https://cmake.org/download/). Ensure CMake is in the PATH.

## Building

Wasmer is built with [Cargo](https://crates.io/), the Rust package manager.

```sh
# checkout code
git clone https://github.com/wasmerio/wasmer.git
cd wasmer

# install tools
# make sure that `python` is accessible.
cargo install --path .
```

## Testing

Thanks to [spectests](https://github.com/wasmerio/wasmer/tree/master/lib/runtime-core/spectests) we can assure 100% compatibility with the WebAssembly spec test suite.

Tests can be run with:

```sh
make test
```

If you need to re-generate the Rust tests from the spectests
you can run:

```sh
make spectests
```

You can also run integration tests with:

```sh
make integration-tests
```

## Roadmap

Wasmer is an open project guided by strong principles, aiming to be modular, flexible and fast. It is open to the community to help set its direction.

Below are some of the goals (written with order) of this project:

- [x] It should be 100% compatible with the [WebAssembly Spectest](https://github.com/wasmerio/wasmer/tree/master/spectests)
- [x] It should be fast _(partially achieved)_
- [ ] Support Emscripten calls _(in the works)_
- [ ] Support Rust ABI calls
- [ ] Support GO ABI calls

## Architecture

If you would like to know how Wasmer works under the hood, please visit our [ARCHITECTURE](https://github.com/wasmerio/wasmer/blob/master/ARCHITECTURE.md) document.

## License

MIT/Apache-2.0

<small>[Attributions](./ATTRIBUTIONS.md)</small>.
