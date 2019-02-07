# SPC-arch
<details>
<summary><b>metadata</b></summary>
<b>partof:</b> <i>none</i></a><br>
<b>parts:</b> <i>none</i></a><br>
<b>file:</b> <a href="https://github.com/vitiral/wasmer/blob/art/architecture/spc.md#L1">architecture/spc.md</a><br>
<b>impl:</b> <a href="https://github.com/vitiral/wasmer/blob/art/src/bin/wasmer.rs#L1">src/bin/wasmer.rs[0]</a><br>
<b>spc:</b>100.00&nbsp;&nbsp;<b>tst:</b>0.00<br>
<hr>
</details>


Wasmer is a binary application which aims to be a universal runtime for wasm.
Currently it is an commandline executable which runs a `.wasm` file like so:

```
wasmer run nginix.wasm
```

See the `README.md` for user documentation and purpose.

Wasmer uses the following components:

- [wabt](https://github.com/pepyakin/wabt-rs): for transforming `.wast` files to `.wasm` and also to run WebAssembly spectests
- [wasmparser](https://github.com/yurydelendik/wasmparser.rs): for parsing the `.wasm` files and translating them into WebAssembly Modules
- [Cranelift](https://github.com/cranestation/cranelift): for compiling WASM function binaries into Machine IR

## High Level Overview

The first time you run `wasmer run <file>`, wasmer will do the following in <a title="/home/rett/open/wasmer/src/bin/wasmer.rs[54]" style="color: #0074D9" href="https://github.com/vitiral/wasmer/blob/art/src/bin/wasmer.rs#L55"><b>.execute_wasm</b></a>:

- Check if `<file>` is a `.wast` file. If so, transform it to `.wasm`
- Check that the provided binary is a valid WebAssembly one. That means, that its binary format starts with `\0asm`.
- If it looks like a WebAssembly file, try to parse it with `webassembly::compile` and generate a `Module` from it
- Create the correct import objects based on whether it is an emscripten file or not. If it is an empscripten file, it will add special imports for it.
- Instantiate the module with the correct imports.
- Try to call the WebAssembly start function, or if unexistent try to search for the one that is exported as `main`.


## Phase 1: Generating the Module / IR

The main entry point is <a title="/home/rett/open/wasmer/lib/runtime/src/lib.rs[125]" style="color: #0074D9" href="https://github.com/vitiral/wasmer/blob/art/lib/runtime/src/lib.rs#L126"><b>.compile</b></a>, but the machinery is really in the default compiler,
the <a title="/home/rett/open/wasmer/lib/clif-backend/src/lib.rs[37]" style="color: #0074D9" href="https://github.com/vitiral/wasmer/blob/art/lib/clif-backend/src/lib.rs#L38"><b>.clif_compiler</b></a>.

As the WebAssembly file is being parsed, it will read the sections in the WebAssembly file (memory, table, function, global and element definitions) using the `ModuleEnv` (<a title="/home/rett/open/wasmer/lib/clif-backend/src/module_env.rs[23]" style="color: #0074D9" href="https://github.com/vitiral/wasmer/blob/art/lib/clif-backend/src/module_env.rs#L24"><b>.module_env</b></a>) as the structure to initial processing and hold this information.

However, the real IR initialization happens while a function body is being parsed/created. That means, when the parser reads the section `(func ...)`.
While the function body is being parsed the corresponding `FuncEnvironment` (<a title="/home/rett/open/wasmer/lib/clif-backend/src/func_env.rs[16]" style="color: #0074D9" href="https://github.com/vitiral/wasmer/blob/art/lib/clif-backend/src/func_env.rs#L17"><b>.func_env</b></a>) methods will be called. This happens in <a title="/home/rett/open/wasmer/lib/clif-backend/src/module_env.rs[386]" style="color: #0074D9" href="https://github.com/vitiral/wasmer/blob/art/lib/clif-backend/src/module_env.rs#L387"><b>.define_function_body</b></a>.

So for example, if the function is using a table, the `make_table` method within that `FuncEnvironment` will be called.
Each of this methods will return the corresponding IR representation.

The `Module` creation will be finished once the parsing is done, and will hold all the function IR as well as the imports/exports.


