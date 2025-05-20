# Building Wasmer from Source

## Installing Rustup

Building Wasmer from source requires [Rust](https://rustup.rs/) **1.81+**.

The easiest way to install Rust on your system is via Rustup. To get Rustup on Linux and macOS, you can run the following:

```bash
curl https://sh.rustup.rs -sSf | sh
```

> [!NOTE]
> To install Rust on Windows, download and run [rustup-init.exe](https://win.rustup.rs/), then follow the on-screen instructions.

## Installing Additional Dependencies

### Linux 
Linux is fully supported by Wasmer. WASI(x) is also fully supported. Users
building from source can enable the LLVM backend following the instruction in
the dedicated section below and installing LLVM version 18. To install it,
refer to [LLVM's download
page](https://github.com/llvm/llvm-project/releases/tag/llvmorg-18.1.7) or
check your distro's package manager.


### macOS
macOS is fully supported by Wasmer. WASI(x) is also fully supported. Users
building from source can enable the LLVM backend following the instruction in
the dedicated section below and installing LLVM version 18. To install it on
macOS, you can use [homebrew](https://brew.sh/): `brew install llvm@18`.


### Windows

Windows is fully supported by Wasmer. WASI(x) is also fully supported.

1. Install [Visual Studio](https://visualstudio.microsoft.com/thank-you-downloading-visual-studio/?sku=Community&rel=15)
2. Install [Rust for Windows](https://win.rustup.rs/)
3. Install [Git for Windows](https://git-scm.com/download/win). Allow it to add `git.exe` to your PATH (default settings for the installer are fine).
4. \(optional\) Install [LLVM 18.0](https://github.com/llvm/llvm-project/releases/download/llvmorg-18.1.7/LLVM-18.1.7-win64.exe)


## Building the Wasmer Runtime

Wasmer is built with [Cargo](https://crates.io/), the Rust package manager.

First, let's clone Wasmer:

```text
git clone https://github.com/wasmerio/wasmer.git
cd wasmer
```

Wasmer supports six different backends at the moment: `singlepass`,
`cranelift`, `LLVM`, `V8`, `wasmi` and `wamr`.

### Singlepass Compiler

The Singlepass compiler works on Linux, Darwin and Windows systems on amd64
platforms and on Linux and Darwin systems on aarch64 platforms. Currently, it
doesn't work on `RISC-V` or `loongarch64`. On system in which it can be used it
is enabled by default.

You can build Wasmer by running this command in the root of the repo:

```text
make build-wasmer
```

**Note**: you should see `singlepass` appear in the `Enabled Compilers: ...` message in the console. 

You may disable the Singlepass backend with the `ENABLE_SINGLEPASS=0` environment
variable, and force its enabling with `ENABLE_SINGLEPASS=1`.

### Cranelift Compiler

The Cranelift compiler will work if you are on a X86 or ARM machine. On system
in which it can be used it is enabled by default.

You can build Wasmer by running this command in the root of the repo:

```text
make build-wasmer
```

**Note**: you should see `cranelift` appear in the `Enabled Compilers: ...` message in the console. 

You may disable the Cranelift backend with the `ENABLE_SINGLEPASS=0` environment
variable, and force its enabling with `ENABLE_SINGLEPASS=1`.

### LLVM Compiler

If you want support for the Wasmer LLVM compiler, then you will also need to:

* Ensure that LLVM >=18.0.x  is installed on your system
  * You can refer to [LLVM install instructions](https://github.com/wasmerio/wasmer/tree/master/lib/compiler-llvm#requirements)
  * You can also [download and use a prebuilt LLVM binary](https://releases.llvm.org/download.html)
* In case `llvm-config` is not accessible, set the correct environment variable
  for LLVM to access: For example, the environment variable for LLVM 18.0.x
  would be: `LLVM_SYS_180_PREFIX=/path/to/unpacked/llvm-18.0`

And create a Wasmer release

```bash
make build-wasmer
```

**Note**: you should see this in the console:  
`Enabled Compilers: llvm`

You may disable the LLVM compiler with `export ENABLE_LLVM=0`.

### V8, wasmi and wamr 
To enable any of these backends, you can set the according `ENABLE_<backend>=1`
flag at build time. The build script itself will download the necessary
libraries at build time.

Note, however, that these backends are not supported on all the platforms that
Wasmer can run on.

For example, to have a Wasmer build with all three backends enabled you can run: 
```text
ENABLE_V8=1 ENABLE_WASMI=1 ENABLE_WAMR=1 make build-wasmer
```

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
