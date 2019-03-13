# Wasmer Architecture

Wasmer uses the following components:

- [Cranelift](https://github.com/cranestation/cranelift): for compiling Wasm binaries to machine code
- [wabt](https://github.com/pepyakin/wabt-rs): for transforming `.wast` files to `.wasm` and running WebAssembly spec tests
- [wasmparser](https://github.com/yurydelendik/wasmparser.rs): for parsing the `.wasm` files and translating them into WebAssembly modules

## How Wasmer works

The first time you run `wasmer run myfile.wasm`, Wasmer will:

- Check if is a `.wast` file, and if so, transform it to `.wasm`
- Check that the provided binary is a valid WebAssembly one, i.e. its binary format starts with `\0asm`.
- Parse it with `wasmparser` and generate a `Module` from it
- Generate an `Instance` with the proper `import_object` (that means, if is detected to be an Emscripten file, it will add the Emscripten expected imports)
- Try to call the WebAssembly `start` function, or if it does not exist, try to search for the function that is exported as `main`

Find a more detailed explanation of the process below:

### Phase 1: Generating the Module / IR

As the WebAssembly file is being parsed, it will read the sections in the WebAssembly file (memory, table, function, global and element definitions) using the `Module` (or `ModuleEnvironment`) as the structure to hold this information.

However, the real IR initialization happens while a function body is being parsed/created, i.e. when the parser reads the section `(func ...)`.
While the function body is being parsed the corresponding `FuncEnvironment` methods will be called.

So for example, if the function is using a table, the `make_table` method within that `FuncEnvironment` will be called.
Each of this methods will return the corresponding IR representation.

The `Module` creation will be finished once the parsing is done, and will hold all the function IR as well as the imports/exports.

### Phase 2: Compiling the Functions

Now that we have a `Module` (and all its definitions living in `ModuleInfo`) we should be ready to compile its functions.

Right now, the `Instance` is the one in charge of compiling this functions into machine code.

When creating the `Instance`, each of the function bodies (IR) will be compiled into machine code that our architecture can understand.
Once we have the compiled values, we will push them to memory and mark them as executable, so we can call them from anywhere in our code.

#### Relocations

Sometimes the functions that we generate will need to call other functions, but the generated code has no idea how to link these functions together.

For example, if a function `A` is calling function `B` (that means is having a `(call b)` on its body) while compiling `A` we will have no idea where the function `B` lives on memory (as `B` is not yet compiled nor pushed into memory).

For that reason, we will start collecting all the calls that function `A` will need to do under the hood, and save it's offsets.
We do that, so we can patch the function calls after compilation, to point to the correct memory address.

Note: sometimes this functions rather than living in the same WebAssembly module, they will be provided as import values.

#### Traps

There will be other times where the function created will cause a trap (for example, if executing `0 / 0`).
When this happens, we will save the offset of the trap (while the function is being compiled).

Thanks to that when we execute a function, if it traps (that means a sigaction is called), we would be able to backtrack from a memory address to a specific trap case.

### Phase 3: Finalizing

Once all the functions are compiled and patched with the proper relocations addresses, we will initialize the corresponding tables (where we save the pointers to all the exported functions), memories and globals that the instance need.

Once that's finished, we will have a `Instance` function that will be ready to execute any function we need.

## Emscripten

Wasmer's Emscripten integration tries to wrap (and emulate) all the different syscalls that Emscripten needs.
We provide this integration by filling the `import_object` with the Emscripten functions, while instantiating the WebAssembly Instance.
