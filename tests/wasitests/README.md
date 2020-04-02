Most of the files here are generated.

`_common.rs` is a file containing a macro that the generated tests use to avoid code duplication.

If you want to add new features, edit `_common.rs` and `wasi-tests/build/wasitests.rs` to use the changed macro.
