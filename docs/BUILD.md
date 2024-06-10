# Building Wasmer from Source

## Installing Rustup

Building Wasmer from source requires [Rust](https://rustup.rs/) **1.67+**.

The easiest way to install Rust on your system is via Rustup. To get Rustup on Linux and macOS, you can run the following:

```bash
curl https://sh.rustup.rs -sSf | sh
```

> [!NOTE]
> To install Rust on Windows, download and run [rustup-init.exe](https://win.rustup.rs/), then follow the on-screen instructions.

## Installing Additional Dependencies

### Windows

Windows is fully supported by Wasmer. WASI is also fully supported, but Emscripten support is still experimental.

1. Install [Visual Studio](https://visualstudio.microsoft.com/thank-you-downloading-visual-studio/?sku=Community&rel=15)
2. Install [Rust for Windows](https://win.rustup.rs/)
3. Install [Git for Windows](https://git-scm.com/download/win). Allow it to add `git.exe` to your PATH (default settings for the installer are fine).
4. \(optional\) Install [LLVM 11.0](https://prereleases.llvm.org/win-snapshots/LLVM-11.0.0-2663a25f-win64.exe)

## Building the Wasmer Runtime

Wasmer is built with [Cargo](https://crates.io/), the Rust package manager.

First, let's clone Wasmer:

```text
git clone https://github.com/wasmerio/wasmer.git
cd wasmer
```

Wasmer supports three different compilers at the moment:

### Singlepass Compiler

Build Wasmer:

```text
make build-wasmer
```

**Note**: you should see this `Enabled Compilers: singlepass` in console. 

You may disable Singlepass compiler with `export ENABLE_SINGLEPASS=0`.

### Cranelift Compiler

The Cranelift compiler will work if you are on a X86 or ARM machine. It will be detected automatically, so you don't need to do anything to your system to enable it.

```text
make build-wasmer
```

**Note**: should see this as the first line in the console:  
`Enabled Compilers: cranelift`

You may disable the Cranelift compiler with `export ENABLE_CRANELIFT=0`.

### LLVM Compiler

If you want support for the Wasmer LLVM compiler, then you will also need to ensure:

* Ensure that LLVM 10.0.x &gt; is installed on your system
  * You can refer to [LLVM install instructions](https://github.com/wasmerio/wasmer/tree/master/lib/compiler-llvm#requirements)
  * You can also [download and use a prebuilt LLVM binary](https://releases.llvm.org/download.html)
* In case `llvm-config` is not accessible, set the correct environment variable for LLVM to access: For example, the environment variable for LLVM 11.0.x would be: `LLVM_SYS_110_PREFIX=/path/to/unpacked/llvm-11.0`

And create a Wasmer release

```bash
make build-wasmer
```

**Note**: you should see this in the console:  
`Enabled Compilers: llvm`

You may disable the LLVM compiler with `export ENABLE_LLVM=0`.

### All compilers

Once you have LLVM and Rust, you can just run:

```bash
make build-wasmer
```

**Note**: you should see this in the console:  
`Enabled Compilers: singlepass cranelift llvm`

## Running your Wasmer binary

Once you run the `make build-wasmer` command, you will have a new binary ready to be used!

```text
./target/release/wasmer quickjs.wasm
```

## Building Wasmer C-API from source

Wasmer provides a pre-compiled version for the C-API on its [release page](https://github.com/wasmerio/wasmer/releases).

However, you can also compile the shared library from source:

```text
make build-capi
```

This will generate the shared library (depending on your system):

* Windows: `target/release/libwasmer_c_api.dll`
* macOS: `target/release/libwasmer_c_api.dylib`
* Linux: `target/release/libwasmer_c_api.so`

If you want to generate the library and headers for using them easily, you can execute:

```bash
make package-capi
```

This command will generate a `package` directory, that you can then use easily in the [Wasmer C API examples](./).

```text
package/
  lib/
    libwasmer.so
  headers/
    wasm.h
    wasmer.h
```

> [!IMPORTANT]
>
> By default, the Wasmer C API shared library will include all the backends available in the system where is built.
> Defaulting to `cranelift` if available.
> 
> You can generate the C-API for a specific compiler and engine with:
> `make build-capi-{ENGINE}`
