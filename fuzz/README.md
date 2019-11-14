This directory contains the fuzz tests for wasmer. To fuzz, we use the `cargo-fuzz` package.

## Installation

You may need to install the `cargo-fuzz` package to get the `cargo fuzz` subcommand. Use

```sh
$ cargo install cargo-fuzz
```

`cargo-fuzz` is documented in the [Rust Fuzz Book](https://rust-fuzz.github.io/book/cargo-fuzz.html).

## Running a fuzzer (simple_instantiate, validate_wasm, compile_wasm)

Once `cargo-fuzz` is installed, you can run the `simple_instantiate` fuzzer with
```sh
cargo fuzz run simple_instantiate
```
or the `validate_wasm` fuzzer
```sh
cargo fuzz run validate_wasm
```
or the `compile_wasm` fuzzer
```sh
cargo fuzz run compile_wasm
```

You should see output that looks something like this:

```
INFO: Seed: 3276026494
INFO:        8 files found in wasmer/fuzz/corpus/simple_instantiate
INFO: -max_len is not provided; libFuzzer will not generate inputs larger than 4096 bytes
INFO: seed corpus: files: 8 min: 1b max: 1b total: 8b rss: 133Mb
#9      INITED ft: 3 corp: 3/3b lim: 4 exec/s: 0 rss: 142Mb
#23     NEW    ft: 4 corp: 4/5b lim: 4 exec/s: 0 rss: 142Mb L: 2/2 MS: 4 ChangeByte-InsertByte-ShuffleBytes-ChangeBit-
#25     NEW    ft: 5 corp: 5/6b lim: 4 exec/s: 0 rss: 142Mb L: 1/2 MS: 2 ChangeBinInt-ChangeBit-
#27     NEW    ft: 6 corp: 6/9b lim: 4 exec/s: 0 rss: 142Mb L: 3/3 MS: 2 InsertByte-ChangeByte-
#190    REDUCE ft: 6 corp: 6/7b lim: 4 exec/s: 0 rss: 142Mb L: 1/2 MS: 3 ChangeBit-EraseBytes-CrossOver-
#205    REDUCE ft: 7 corp: 7/11b lim: 4 exec/s: 0 rss: 142Mb L: 4/4 MS: 5 ShuffleBytes-CrossOver-InsertByte-ChangeBinInt-CrossOver-
```
It will continue to generate random inputs forever, until it finds a bug or is terminated. The testcases for bugs it finds go into `fuzz/artifacts/simple_instantiate` and you can rerun the fuzzer on a single input by passing it on the command line `cargo fuzz run simple_instantiate my_testcase.wasm`.

## Seeding the corpus, optional

The fuzzer works best when it has examples of small Wasm files to start with. Using `wast2json` from [wabt](https://github.com/WebAssembly/wabt), we can easily produce `.wasm` files out of the WebAssembly spec tests.

```sh
mkdir spec-test-corpus
for i in lib/spectests/spectests/*.wast; do wast2json --enable-all $i -o spec-test-corpus/$(basename $i).json; done
mv spec-test-corpus/*.wasm fuzz/corpus/simple_instantiate/
rm -r spec-test-corpus
```

The corpus directory is created on the first run of the fuzzer. If it doesn't exist, run it first and then seed the corpus. The fuzzer will pick up new files added to the corpus while it is running.

## Trophy case

- [x] https://github.com/wasmerio/wasmer/issues/558
