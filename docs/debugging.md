# Debugging Wasmer

## When is this document useful?

If you're developing wasmer or running into issues, this document will explain to useful techniques and common errors.

## Tracing syscalls

To trace syscalls, compile with the `debug` feature (`cargo build --features "debug"`).  For even more verbose messages, use the `trace` flag.

## Tracing calls

TODO: did we disable tracing calls? if not talk about how to enable it
TODO: someone with more context on the backends mention which backends this works for

If you'd like to see all calls and you're using emscripten, you can use a symbol map to get better error output with the `em-symbol-map` flag.

## Common things that can go wrong

### Missing imports

If, when attempting to run a wasm module, you get an error about missing imports there are a number of things that could be going wrong.

The most likely is that we haven't implemented those imports for your ABI.  If you're targeting emscripten, this is probably the issue.

However if that's not the case, then there's a chance that you're using an unsupported ABI (let us know!) or that the wasm is invalid for the detected ABI.  (TODO: link to wasm contracts or something)

### Hitting `undefined`

If this happens it's because wasmer does not have full support for whatever feature you tried to use.  Running with tracing on can help clarify the issue if it's not clear from the message.

To fix this, file an issue letting us know that wasmer is missing a feature that's important to you.  If you'd like, you can try to implement it yourself and send us a PR.

### No output

If you're seeing no output from running the wasm module then it may be that:
- this is the intended behavior of the wasm module
- or it's very slow to compile (try compiling with a faster backend like cranelift (the default) or singlepass (requires nightly))

### Segfault

If you're seeing a segfault while developing wasmer, chances are that it's a cache issue.  We reset the cache on every version bump, but if you're running it from source then the cache may become invalid, which can lead to segfaults.

To fix this delete the cache with `wasmer cache clean` or run the command with the `disable-cache` flag (`wasmer run some.wasm --disable-cache`)

If you're seeing a segfault with a released version of wasmer, please file an issue so we can ship an updated version as soon as possible.

### Something else

If none of this has helped with your issue, let us know and we'll do our best to help.
