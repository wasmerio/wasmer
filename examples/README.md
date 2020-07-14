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
   $ cargo run --features "cranelift" --example engine-jit
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
   $ cargo run --features "cranelift" --example engine-native
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
   $ cargo run --features "cranelift" --example engine-headless
   ```

   </details>

4. [**Cross-compilation**][cross-compilation], illustrates the power
   of the abstraction over the engines and the compilers, such as it
   is possible to cross-compile a Wasm module.
   
   _Keywords_: engine, compiler, cross-compilation.

   <details>
   <summary><em>Execute the example</em></summary>

   ```shell
   $ cargo run --features "cranelift" --example cross-compilation
   ```

   </details>
   
### Exports
   
5. [**Exported function**][exported-function], explains how to get and
   how to call an exported function. They come in 2 flavors: dynamic,
   and “static”/native. The pros and cons are discussed briefly.
   
   _Keywords_: export, function, dynamic, static, native.

   <details>
   <summary><em>Execute the example</em></summary>

   ```shell
   $ cargo run --features "cranelift" --example exported-function
   ```

   </details>


[engine-jit]: ./engine_00_jit.rs
[engine-native]: ./engine_01_native.rs
[engine-headless]: ./engine_02_headless.rs
[cross-compilation]: ./engine_03_cross_compilation.rs
[exported-function]: ./exports_00_function.rs
