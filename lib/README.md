# Wasmer Libraries

Wasmer is modularized into different libraries, separated into three main sections:

- [Runtime](#runtime)
- [Integrations](#integrations)
- [Backends](#backends)

## Runtime

The core of Wasmer is the runtime, which provides the necessary
abstractions to create a good user experience when embedding.

The runtime is divided into two main libraries:

- [runtime-core](./runtime-core/): The main implementation of the runtime.
- [runtime](./runtime/): Easy-to-use API on top of `runtime-core`.

## Integrations

The integration builds on the Wasmer runtime and allow us to run WebAssembly files compiled for different environments.

Wasmer intends to support different integrations:

- [emscripten](./emscripten): run Emscripten-generated WebAssembly files, such as [Lua](../examples/lua.wasm) or [nginx](../examples/nginx/nginx.wasm).
- Go ABI: _we will work on this soon! Want to give us a hand? ✋_
- Blazor: _research period, see [tracking issue](https://github.com/wasmerio/wasmer/issues/97)_

## Backends

The Wasmer [runtime](./runtime) is designed to support multiple compiler backends, allowing the user
to tune the codegen properties (compile speed, performance, etc) to best fit their use case.

Currently, we support a Cranelift compiler backend:

- [clif-backend](./clif-backend/): The integration of Wasmer with Cranelift
