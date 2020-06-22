# Wasmer Compiler

This crate is the base for Compiler implementations.

It performs the translation from a Wasm module into a basic ModuleInfo,
but leaves the Wasm function bytecode translation to the compiler implementor.

Here are some of the Compilers provided by Wasmer:
* [Singlepass](https://github.com/wasmerio/wasmer-reborn/tree/master/lib/compiler-singlepass)
* [Cranelift](https://github.com/wasmerio/wasmer-reborn/tree/master/lib/compiler-cranelift)
* [LLVM](https://github.com/wasmerio/wasmer-reborn/tree/master/lib/compiler-llvm)

## How to create a compiler

Creating a new compiler is quite easy, you just need to impement two traits: `CompilerConfig` and `Compiler`:

```rust
/// The compiler configuration options.
pub trait CompilerConfig {
    /// Gets the custom compiler config
    fn compiler(&self) -> Box<dyn Compiler + Send>;
}

/// An implementation of a Compiler from parsed WebAssembly module to Compiled native code.
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


### Acknowledgments

This project borrowed some of the code strucutre from the [cranelift-wasm](https://crates.io/crates/cranelift-wasm) crate, however it's been adapted to not depend on any specific IR and be abstract of any compiler.

Please check [Wasmer ATTRIBUTIONS](https://github.com/wasmerio/wasmer/blob/master/ATTRIBUTIONS.md) to further see licenses and other attributions of the project. 
