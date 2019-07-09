This directory contains tests for unit testing each of the functions/syscalls that Emscripten will be doing.

If you want to generate the wasm files, you will just need:

Have EMCC (version 1.38.21) installed

Then run:

```
make emtests
```

**Ignored Tests**
Test names included in `emtests/ignores.txt` will be annotated with `#[ignore]` during test generation.

This process will do something similar to:

```
# Generate the .wasm file
emcc localtime.c -o localtime.js
# Delete the js file, as we don't need it
rm localtime.js
```
