# Wasmer Native

The Wasmer Native is usable with any compiler implementation
based on `wasmer-compiler` that is able to emit Position Independent
Code (PIC).

After the compiler process the result, the Native Engine generates
a shared object file and links it via `dlsym` so it can be usable by the
`wasmer` api.
