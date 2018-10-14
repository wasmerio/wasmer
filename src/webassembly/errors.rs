error_chain! {
    errors {
        //! The webassembly::CompileError() constructor creates a new WebAssembly
        //! CompileError object, which indicates an error during WebAssembly
        //! decoding or validation
        CompileError(reason: String) {
            description("WebAssembly compilation error")
            display("Compilation error: '{:?}'", reason)
        }

        //! The webassembly::LinkError() constructor creates a new WebAssembly
        //! LinkError object, which indicates an error during module instantiation
        //! (besides traps from the start function).
        LinkError(reason: String) {
            description("WebAssembly link error")
            display("Link error: '{:?}'", reason)
        }

        // The webassembly::RuntimeError() constructor creates a new WebAssembly
        // RuntimeError object — the type that is thrown whenever WebAssembly
        //  specifies a trap.
        RuntimeError(reason: String) {
            description("WebAssembly runtime error")
            display("Runtime error: '{:?}'", reason)
        }
    }
}
