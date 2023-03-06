# Using Wasm coredump

The following steps describe how to debug using Wasm coredump in Wasmer:

1. Compile your WebAssembly with debug info enabled; for example: 

    ```sh
    $ rustc foo.rs --target=wasm32-wasi -C debuginfo=2
    ```

<details>
    <summary>foo.rs</summary>

    fn c(v: usize) {
        a(v - 3);
    }

    fn b(v: usize) {
        c(v - 3);
    }

    fn a(v: usize) {
        b(v - 3);
    }

    pub fn main() {
        a(10);
    }
</details>

2. Run with Wasmer and Wasm coredump enabled:

    ```sh
    $ wasmer --coredump-on-trap=/tmp/coredump foo.wasm

    thread 'main' panicked at 'attempt to subtract with overflow', foo.rs:10:7
    note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
    Error: failed to run main module `foo.wasm`

    Caused by:
        0: Core dumped at /tmp/coredump
        1: failed to invoke command default
        2: error while executing at wasm backtrace:
                    ...
    ```

3. Use [wasmgdb] to debug:
    ```sh
    $ wasmgdb foo.wasm /tmp/coredump

    wasmgdb> bt
    ...
    #13     000175 as panic () at library/core/src/panicking.rs
    #12     000010 as a (v=???) at /path/to/foo.rs
    #11     000009 as c (v=???) at /path/to/foo.rs
    #10     000011 as b (v=???) at /path/to/foo.rs
    #9      000010 as a (v=???) at /path/to/foo.rs
    #8      000012 as main () at /path/to/foo.rs
    ...
    ```

[wasmgdb]: https://crates.io/crates/wasmgdb
