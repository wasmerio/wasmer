//! The webassembly::CompileError() constructor creates a new WebAssembly
//! CompileError object, which indicates an error during WebAssembly
//! decoding or validation

//! The webassembly::LinkError() constructor creates a new WebAssembly
//! LinkError object, which indicates an error during module instantiation
//! (besides traps from the start function).

//! The webassembly::RuntimeError() constructor creates a new WebAssembly
//! RuntimeError object — the type that is thrown whenever WebAssembly
//!  specifies a trap.

error_chain! {
    errors {
        CompileError(reason: String) {
            description("WebAssembly compilation error")
            display("Compilation error: {}", reason)
        }

        LinkError(reason: String) {
            description("WebAssembly link error")
            display("Link error: {}", reason)
        }

        RuntimeError(reason: String) {
            description("WebAssembly runtime error")
            display("Runtime error: {}", reason)
        }
    }
}
