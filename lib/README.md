# Wasmer Libraries

Wasmer is modularized into different libraries, separated into three main sections:
* [Runtime](#Runtime)
* [Integrations](#Integrations)
* [Backends](#Backends)

## Runtime

The core of Wasmer is the runtime.
It provides the necessary abstractions on top of the WebAssembly specification.

We separated the runtime into two main libraries:
* [runtime-core](./runtime-core/): The main implementation of the runtime
* [runtime](./runtime/): Easy-to-use wrappers on top of runtime-core

## Integrations

The integrations are separated implementations in top of our runtime, that let Wasmer run more WebAssembly files.

Wasmer intends to support different integrations:
* [emscripten](./emscripten): it let us run emscripten-generated WebAssembly files, such as lua or Nginx.
* Go ABI: _we will work on this soon! Would you like to help us? 💪_
* Blazor: _researching period, see [tracking issue](https://github.com/wasmerio/wasmer/issues/97)_


## Backends

The backends let Wasmer generate code from WebAssembly files, in a way that is abstracted from the 
IR library itself.

* [clif-backend](./clif-backend/): The integration of Wasmer with Cranelift
