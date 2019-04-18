# Dockerfile Documentation
The `Dockerfile` included in the project root directory could be used for development purposes or to build a small image containing the `wasmer` executable.

The `wasmer-build-env` stage in the Dockerfile contains the dependencies needed to compile Wasmer including LLVM.

The `wasmer-debug-env` stage adds the `valgrind` profiling tool to the `wasmer-build-env` stage.

The `wasmer-build` stage in the Dockerfile will copy the current directory, assuming the build context is the `wasmer` project, and build the project using `cargo build --release`.

The `wasmer` stage will copy the resulting `wasmer` executable from the `wasmer-build` stage into a new base image to create a smaller image containing `wasmer`.

## Example Usages

### Wasmer image
1. From the `wasmer` project directory, build the image:
`docker build -t wasmer --target=wasmer .`

2. List options:
`docker run wasmer --help`

3. Mount a directory, and run an example wasm file:
`docker run -v /Users/admin/Documents/wasmer-workspace:/root/wasmer-workspace wasmer run /root/wasmer-workspace/examples/hello.wasm`

### Profiling
1. Build `wasmer-debug-env`:
   `docker build --tag=wasmer-debug-env --target wasmer-debug-env .`

2. Mount a directory from the host and run interactively:
   `docker run -it -v /Users/admin/Documents/wasmer-workspace:/home/circleci/wasmer-workspace wasmer-debug-env /bin/bash`

3. Inside the container, build `wasmer` and run profiling tool:
```
cd /home/circleci/wasmer-workspace/wasmer`
cargo build
valgrind --tool=callgrind --dump-instr=yes --collect-jumps=yes --simulate-cache=yes target/debug/wasmer run test.wasm
```

The `callgrind.out` can be viewed with the `qcachegrind` tool on Mac OS (`brew install qcachegrind`).

