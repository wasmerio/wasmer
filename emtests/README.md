This directory contains tests for unit testing each of the functions/syscalls that Emscripten will be doing.

If you want to generate the wasm files, you will just need to:

```
make emtests
```

This process will do something similar to:

```
cc localtime.c -o localtime.out
# Execute the out file and save its output
./localtime.out > ./localtime.output
rm localtime.out

# Generate the .wasm file
emcc localtime.c -o localtime.js
# Delte the js file, as we don't need it
rm localtime.js
```
