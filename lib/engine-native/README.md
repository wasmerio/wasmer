# Wasmer Native

The Wasmer Native is usable with any compiler implementation
based on `wasmer-compiler` that is able to emit Position Independent
Code (PIC).

After the compiler generates the machine code for the functions, the
Native Engine generates a shared object file and links it via `dlsym`
so it can be usable by the `wasmer` API.

This allows Wasmer to achieve *blazing fast* native startup times.

*Note: you can find a [full working example using the JIT engine here](https://github.com/wasmerio/wasmer-reborn/blob/master/examples/engine-jit.rs).*

## Requirements

The `wasmer-engine-native` crate requires a linker available on your
system to generate the shared object file.

We recommend having `gcc` or `clang` installed.

> Note: when cross-compiling to other targets, `clang` will be the
> default command used for compiling.

You can install LLVM easily on your debian-like system via this command:

```bash
bash -c "$(wget -O - https://apt.llvm.org/llvm.sh)"
```

Or in macOS:

```bash
brew install llvm
```

Or via any of the [pre-built binaries that LLVM offers](https://releases.llvm.org/download.html).
