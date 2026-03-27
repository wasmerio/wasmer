# Testing

Thanks to [WebAssembly spec tests](https://github.com/WebAssembly/testsuite) we can ensure 100% compatibility with the WebAssembly spec test suite.

You can run all the tests with:

```text
make test
```

> [!NOTE]
> `make test` will automatically detect the compilers available on your system.
> 
> Please follow the [Building from Source](./BUILD.md) guide see how you can[ prepare your system with the requirements needed for each of the backends](./#all-backends-default).

## Testing Compilers

Each compiler integration can be tested separately:

* **Singlepass**: `make test-singlepass`
* **Cranelift**: `make test-cranelift`
* **LLVM**: `make test-llvm`
