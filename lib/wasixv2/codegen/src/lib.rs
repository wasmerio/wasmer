//! Build-time generator. From the single syscall table it emits the wasm import shims, argument
//! decoders, the `Syscalls` trait, the request enum and the forwarding `Layered` adapter.
