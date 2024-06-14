> [!WARNING]
> The WAMR backend is, as of now, an experimental feature. This document is
> likely to change quickly.

# Introduction

Recent efforts introduced the possibility to use the WAMR interpreter as a
backend in Wasmer. Among other things, this allows Wasmer to be used in iOS!
This document has the objective to document important aspects of this backend.

## About the WAMR backend
1. Direct calls (outside host) to grow memory are [not
   supported](https://github.com/bytecodealliance/wasm-micro-runtime/blob/main/doc/memory_tune.md#the-memory-model),
   and users are encouraged to use the dedicated opcode (`memory.grow`)
   instead.
2. Importing memories and tables is not supported. If used, WAMR will simply
   print `doesn't support import memories and tables for now, ignore them` to
   standard output.
3. Due to point (2) above, multithreaded programs relying on importing memories
   can fail with out-of-bound memory read/write errors.
4. Custom sections are not entirely supported as of now. 
5. Functions cannot be called without an attached instance. 
6. Globals cannot be inspected without an attached instance. 

Notice, again, that the support is experimental: if you happen to incur in
other issues not listed here, please file an issue!
