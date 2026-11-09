# `wasmer-asyncify`

Drive WebAssembly modules instrumented with
[asyncify](https://emscripten.org/docs/porting/asyncify.html) from Wasmer.

An asyncified module can unwind its stack back to the host and later be
rewound to where it left off. This crate gives you:

- `Asyncify`: the module's `asyncify_*` exports and the data buffer they use,
  with `start_unwind`, `stop_unwind`, `start_rewind`, `stop_rewind` and
  `state`.
- `AsyncifyFunction::call_asyncify` (and `AsyncifyCallWrapper` for typed
  functions): calls into the module and, once it has unwound, runs the
  `OnCalledHandler` a host function left on its environment, re-entering the
  function when the handler rewinds.

A host function suspends the guest like this:

1. Normal state: call `Asyncify::start_unwind`, leave an `OnCalledHandler` on
   the environment and return.
2. The handler runs once the guest has unwound. It calls
   `Asyncify::stop_unwind`, does its work, calls `Asyncify::start_rewind` and
   returns `OnCalledAction::InvokeAgain`.
3. The guest rewinds into the host function, which sees the `Rewinding` state,
   calls `Asyncify::stop_rewind` and returns its result.

`tests/sleep.rs` runs exactly this against a module produced by
`wasm-opt --asyncify`. WASIX builds fork, exec, `longjmp` and deep sleep on
this crate.
