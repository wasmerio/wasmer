# Wasmer C API

This is the [Wasmer WebAssembly Runtime](https://wasmer.io) shared library.
You can use it in any C/C++ projects.

This directory is structured like the following:
* `lib` is where the Wasmer shared library lives.
* `include` is where the Wasmer headers live

## Using it

If you want to compile a `c` file using Wasmer, you can do:

```bash
clang YOUR_FILE -Iinclude -lwasmer -Llib
```

> Note: append ` -rpath lib` if you are in macOS.

## Examples

You can check examples of how to use the Wasmer C API here:

https://docs.wasmer.io/integrations/c/examples

