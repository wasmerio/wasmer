This directory contains tests for unit testing each of the functions/syscalls that Emscripten will be doing.

If you want to generate the wasm files, you will just need to:

```
emcc localtime.c -o localtime.js
# Delte the js file, as we don't need it
rm localtime.js
```
