# Wasmer Compiler

This crate is the base for Compiler implementations.

It performs the translation from a Wasm module into a basic
`ModuleInfo`, but leaves the Wasm function bytecode translation to the
compiler implementor.

Here are some of the Compilers provided by Wasmer:

* [Singlepass](https://github.com/wasmerio/wasmer-reborn/tree/master/lib/compiler-singlepass),
* [Cranelift](https://github.com/wasmerio/wasmer-reborn/tree/master/lib/compiler-cranelift),
* [LLVM](https://github.com/wasmerio/wasmer-reborn/tree/master/lib/compiler-llvm).

## How to create a compiler

To create a compiler, one needs to implement two traits:

1. `CompilerConfig`, that configures and creates a new compiler,
2. `Compiler`, the compiler itself that will compile a module.

```rust
/// The compiler configuration options.
pub trait CompilerConfig {
    /// Gets the custom compiler config
    fn compiler(&self) -> Box<dyn Compiler + Send>;
}

/// An implementation of a compiler from parsed WebAssembly module to compiled native code.
pub trait Compiler {
    /// Compiles a parsed module.
    ///
    /// It returns the [`Compilation`] or a [`CompileError`].
    fn compile_module<'data, 'module>(
        &self,
        target: &Target,
        compile_info: &'module CompileModuleInfo,
        module_translation: &ModuleTranslationState,
        // The list of function bodies
        function_body_inputs: PrimaryMap<LocalFunctionIndex, FunctionBodyData<'data>>,
    ) -> Result<Compilation, CompileError>;
}
```
