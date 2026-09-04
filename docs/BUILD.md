# Building Wasmer from Source

## Installing Rustup

Wasmer supports building to the latest **3** stable releases, but pins
to one specific one at the time. `rustup` selects it automatically. See
[rust-toolchain.toml](../rust-toolchain.toml) for the current pin.
The easiest way to install Rust on your system is via [Rustup](https://rustup.rs/). To get Rustup on Linux and macOS, you can run the following:

```bash
curl https://sh.rustup.rs -sSf | sh
```

> [!NOTE]
> To install Rust on Windows, download and run [rustup-init.exe](https://win.rustup.rs/), then follow the on-screen instructions.

## Installing Additional Dependencies

### Linux

Linux is fully supported by Wasmer. WASI(x) is also fully supported. Users
building from source can enable the LLVM backend following the instruction in
the dedicated section below and installing LLVM version 22. To install it,
refer to [LLVM's download
page](https://github.com/llvm/llvm-project/releases/tag/llvmorg-22.1.1) or
check your distro's package manager.

### macOS

macOS is fully supported by Wasmer. WASI(x) is also fully supported. Users
building from source can enable the LLVM backend following the instruction in
the dedicated section below and installing LLVM version 22. To install it on
macOS, you can use [homebrew](https://brew.sh/): `brew install llvm@22`.

### Windows

Windows is fully supported by Wasmer. WASI(x) is also fully supported.

1. Install [Visual Studio](https://visualstudio.microsoft.com/thank-you-downloading-visual-studio/?sku=Community&rel=15)
2. Install [Rust for Windows](https://win.rustup.rs/)
3. Install [Git for Windows](https://git-scm.com/download/win). Allow it to add `git.exe` to your PATH (default settings for the installer are fine).
4. \(optional\) Install [LLVM 22.1](https://github.com/llvm/llvm-project/releases/download/llvmorg-22.1.1/LLVM-22.1.1-win64.exe)

## Building the Wasmer Runtime

Wasmer is built with [Cargo](https://crates.io/), the Rust package manager.

For reproducible builds, set `WASMER_REPRODUCIBLE_BUILD=1` in the build
environment. This removes the build timestamp from `wasmer --version -v`
by omitting the verbose `commit-date:` line.

First, let's clone Wasmer along with its submodules:

```text
git clone --recursive https://github.com/wasmerio/wasmer.git
cd wasmer
```

In an existing clone, initialize the submodules with
`git submodule update --init --recursive`.

Wasmer supports different backends at the moment: `singlepass`, `cranelift`, `LLVM` and `V8`.

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

You may disable the Cranelift backend with the `ENABLE_CRANELIFT=0` environment
variable, and force its enabling with `ENABLE_CRANELIFT=1`.

### LLVM Compiler

If you want support for the Wasmer LLVM compiler, then you will also need to:

- Ensure that LLVM 22 (>=22.1.x) is installed on your system. The backend
  needs LLVM 22 exactly; any other version silently disables it — read the
  `Enabled Compilers:` banner. The error `Didn't find usable system-wide
  LLVM` means LLVM 22 is missing.
  - You can refer to [LLVM install instructions](https://github.com/wasmerio/wasmer/tree/master/lib/compiler-llvm#requirements)
  - You can also [download and use a prebuilt LLVM binary](https://releases.llvm.org/download.html)
- In case `llvm-config-22` is not on PATH, set the correct environment variable
  for LLVM to access: For example, the environment variable for LLVM 22.1.x
  would be: `LLVM_SYS_221_PREFIX=/path/to/unpacked/llvm-22.1`

And create a Wasmer release

```bash
make build-wasmer
```

**Note**: you should see this in the console:  
`Enabled Compilers: llvm`

You may disable the LLVM compiler with `export ENABLE_LLVM=0`.

### V8

To enable the backend, you can set the according `ENABLE_<backend>=1`
flag at build time. The build script itself will download the necessary
libraries at build time.

Note, however, that these backends are not supported on all the platforms that
Wasmer can run on.

```text
ENABLE_V8=1 make build-wasmer
```

### All Compilers

Once you have LLVM and Rust, you can just run:

```bash
make build-wasmer
```

**Note**: you should see this in the console:  
`Enabled Compilers: singlepass cranelift llvm`

## Iterating During Development

For fast iteration, run `make check`, or build one crate:

```bash
cargo build -p wasmer-cli --features cranelift
```

`make build-wasmer-debug` builds a debug binary with tokio-console support.

Read the `Enabled Compilers:` banner that each make target prints. The
Makefile silently omits backends it cannot detect. V8 is never
autodetected.

The wasix-libc sysroot and Rust toolchain pins for CI live in
`.github/ci-constants.env`.

> [!CAUTION]
> Do not build with `cargo build --workspace --features <backend>`.
> Workspace-level features do not reach subcrates. The result is a headless
> binary that cannot compile Wasm. Use `-p wasmer-cli` or the Makefile.

## Running Your Wasmer Binary

Once you run the `make build-wasmer` command, you will have a new binary ready to be used!

```text
./target/release/wasmer quickjs.wasm
```

## Building Wasmer C-API from Source

Wasmer provides a pre-compiled version for the C-API on its [release page](https://github.com/wasmerio/wasmer/releases).

However, you can also compile the shared library from source:

```text
make build-capi
```

This will generate the shared library (depending on your system):

- Windows: `target/release/libwasmer_c_api.dll`
- macOS: `target/release/libwasmer_c_api.dylib`
- Linux: `target/release/libwasmer_c_api.so`

If you want to generate the library and headers for using them easily, you can execute:

```bash
make package-capi
```

This command will generate a `package` directory, that you can then use easily in the [Wasmer C API examples](./).

```text
package/
  lib/
    libwasmer.so
  include/
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
