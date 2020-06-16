# Wasmer Compiler - LLVM

This is the `wasmer-compiler-llvm` crate, which contains a
compiler implementation based on LLVM.

We recommend using LLVM as the default compiler when running WebAssembly
files on any **production** system, as it offers maximum peformance near
to native speeds.

## Requirements

The llvm compiler requires a valid installation of LLVM in your system.
It currently requires **LLVM 10**.


You can install LLVM easily on your debian-like system via this command:

```bash
bash -c "$(wget -O - https://apt.llvm.org/llvm.sh)"
```

Or in macOS:

```bash
brew install llvm
```

Or via any of the [pre-built binaries that LLVM offers](https://releases.llvm.org/download.html).
