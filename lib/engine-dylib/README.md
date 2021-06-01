<div align="center">
  <a href="https://wasmer.io" target="_blank" rel="noopener noreferrer">
    <img width="300" src="https://raw.githubusercontent.com/wasmerio/wasmer/master/assets/logo.png" alt="Wasmer logo">
  </a>

  <h1>The <code>wasmer-engine-dylib</code> library</h1>

  <p>
    <a href="https://github.com/wasmerio/wasmer/actions?query=workflow%3Abuild">
      <img src="https://github.com/wasmerio/wasmer/workflows/build/badge.svg?style=flat-square" alt="Build Status" />
    </a>
    <a href="https://github.com/wasmerio/wasmer/blob/master/LICENSE">
      <img src="https://img.shields.io/github/license/wasmerio/wasmer.svg?style=flat-square" alt="License" />
    </a>
    <a href="https://slack.wasmer.io">
      <img src="https://img.shields.io/static/v1?label=Slack&message=join%20chat&color=brighgreen&style=flat-square" alt="Slack channel" />
    </a>
    <a href="https://crates.io/crates/wasmer-engine-dylib">
      <img src="https://img.shields.io/crates/v/wasmer-engine-dylib.svg?style=flat-square" alt="crates.io" />
    </a>
    <a href="https://wasmerio.github.io/wasmer/crates/wasmer_engine_dylib/">
      <img src="https://img.shields.io/badge/documentation-read-informational?style=flat-square" alt="documentation" />
    </a>
  </p>
</div>

<br />

The Wasmer Dylib engine is usable with any compiler implementation
based on [`wasmer-compiler`] that is able to emit
[Position-Independent Code][PIC] (PIC).

After the compiler generates the machine code for the functions, the
Dylib Engine generates a shared object file and links it via [`dlsym`]
so it can be usable by the [`wasmer`] API.

This allows Wasmer to achieve *blazing fast* **native startup times**.

*Note: you can find a [full working example using the Dylib engine
here][example].*

### Difference with `wasmer-engine-universal`

The Dylib Engine and Universal Engine mainly differ on how the Modules
are loaded/stored. Using the same compilers, both will have the same
runtime speed.

However, the Dylib Engine uses the Operating System shared library
loader (via `dlopen`) and as such is able to achieve a much faster
startup time when deserializing a serialized `Module`.

## Requirements

The `wasmer-engine-dylib` crate requires a linker available on
your system to generate the shared object file.

We recommend having [`gcc`] or [`clang`] installed.

> Note: when **cross-compiling** to other targets, `clang` will be the
> default command used for compiling.

You can install LLVM (that provides `clang`) easily on your
Debian-like system via this command:

```bash
bash -c "$(wget -O - https://apt.llvm.org/llvm.sh)"
```

Or in macOS:

```bash
brew install llvm
```

Or via any of the [pre-built binaries that LLVM
offers][llvm-pre-built].


[`wasmer-compiler`]: https://github.com/wasmerio/wasmer/tree/master/lib/compiler
[PIC]: https://en.wikipedia.org/wiki/Position-independent_code
[`dlsym`]: https://www.freebsd.org/cgi/man.cgi?query=dlsym
[`wasmer`]: https://github.com/wasmerio/wasmer/tree/master/lib/api
[example]: https://github.com/wasmerio/wasmer/blob/master/examples/engine_dylib.rs
[`gcc`]: https://gcc.gnu.org/
[`clang`]: https://clang.llvm.org/
[llvm-pre-built]: https://releases.llvm.org/download.html
