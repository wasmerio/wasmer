# Wasmer Examples

This directory contains a collection of examples. This isn't an
exhaustive collection though, if one example is missing, please ask,
we will be happy to fulfill your needs!

## Examples

The examples are written in a difficulty/discovery order. Concepts that
are explained in an example is not necessarily re-explained in a next
example.

### Engines

1. [**JIT engine**][engine-jit], explains what an engine is, what the
   JIT engine is, and how to set it up. The example completes itself
   with the compilation of the Wasm module, its instantiation, and
   finally, by calling an exported function.
   
   _Keywords_: JIT, engine, in-memory, executable code.
   
   <details>
   <summary><em>Execute the example</em></summary>

   ```shell
   $ cargo run --example engine-jit --release --features "cranelift"
   ```

   </details>

2. [**Native engine**][engine-native], explains what a native engine
   is, and how to set it up. The example completes itself with the
   compilation of the Wasm module, its instantiation, and finally, by
   calling an exported function.
   
   _Keywords_: native, engine, shared library, dynamic library,
   executable code.

   <details>
   <summary><em>Execute the example</em></summary>

   ```shell
   $ cargo run --example engine-native --release --features "cranelift"
   ```

   </details>

3. [**Headless engines**][engine-headless], explains what a headless
   engine is, what problem it does solve, and what are the benefits of
   it. The example completes itself with the instantiation of a
   pre-compiled Wasm module, and finally, by calling an exported
   function.
   
   _Keywords_: native, engine, constrained environment, ahead-of-time
   compilation, cross-compilation, executable code, serialization.

   <details>
   <summary><em>Execute the example</em></summary>

   ```shell
   $ cargo run --example engine-headless --release --features "cranelift"
   ```

   </details>

4. [**Cross-compilation**][cross-compilation], illustrates the power
   of the abstraction over the engines and the compilers, such as it
   is possible to cross-compile a Wasm module for a custom target.
   
   _Keywords_: engine, compiler, cross-compilation.

   <details>
   <summary><em>Execute the example</em></summary>

   ```shell
   $ cargo run --example cross-compilation --release --features "cranelift"
   ```

   </details>

### Compilers

5. [**Singlepass compiler**][compiler-singlepass], explains how to use
   the [`wasmer-compiler-singlepass`] compiler.
   
   _Keywords_: compiler, singlepass.

   <details>
   <summary><em>Execute the example</em></summary>

   ```shell
   $ cargo run --example compiler-singlepass --release --features "singlepass"
   ```

   </details>

6. [**Cranelift compiler**][compiler-cranelift], explains how to use
   the [`wasmer-compiler-cranelift`] compiler.
   
   _Keywords_: compiler, cranelift.

   <details>
   <summary><em>Execute the example</em></summary>

   ```shell
   $ cargo run --example compiler-cranelift --release --features "cranelift"
   ```

   </details>

7. [**LLVM compiler**][compiler-llvm], explains how to use the
   [`wasmer-compiler-llvm`] compiler.
   
   _Keywords_: compiler, llvm.

   <details>
   <summary><em>Execute the example</em></summary>

   ```shell
   $ cargo run --example compiler-llvm --release --features "llvm"
   ```

   </details>

### Exports
   
8. [**Exported function**][exported-function], explains how to get and
   how to call an exported function. They come in 2 flavors: dynamic,
   and “static”/native. The pros and cons are discussed briefly.
   
   _Keywords_: export, function, dynamic, static, native.

   <details>
   <summary><em>Execute the example</em></summary>

   ```shell
   $ cargo run --example exported-function --release --features "cranelift"
   ```

   </details>

### Integrations

9. [**WASI**][wasi], explains how to use the [WebAssembly System
   Interface][WASI] (WASI), i.e. the [`wasmer-wasi`] crate.
   
   _Keywords_: wasi, system, interface

   <details>
   <summary><em>Execute the example</em></summary>

   ```shell
   $ cargo run --example wasi --release --features "cranelift,wasi"
   ```

   </details>

[engine-jit]: ./engine_jit.rs
[engine-native]: ./engine_native.rs
[engine-headless]: ./engine_headless.rs
[compiler-singlepass]: ./compiler_singlepass.rs
[compiler-cranelift]: ./compiler_cranelift.rs
[compiler-llvm]: ./compiler_llvm.rs
[cross-compilation]: ./engine_cross_compilation.rs
[exported-function]: ./exports_function.rs
[wasi]: ./wasi.rs
[`wasmer-compiler-singlepass`]: https://github.com/wasmerio/wasmer-reborn/tree/master/lib/compiler-singlepass
[`wasmer-compiler-cranelift`]: https://github.com/wasmerio/wasmer-reborn/tree/master/lib/compiler-cranelift
[`wasmer-compiler-llvm`]: https://github.com/wasmerio/wasmer-reborn/tree/master/lib/compiler-llvm
[`wasmer-wasi`]: https://github.com/wasmerio/wasmer-reborn/tree/master/lib/wasi
[WASI]: https://github.com/WebAssembly/WASI
