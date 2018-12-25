# Run Nginx

This example has Nginx compiled to WebAssembly using Emscripten.

You can run it locally with:

```
wasmer run nginx.wasm -- -p . -c nginx.conf
```

And you will have a webserver running in:
http://localhost:8080/
