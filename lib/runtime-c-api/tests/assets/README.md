These are used in tests in the parent directory.

To keep the generated wasm small, use `wasm-opt` and `wasm-strip` from wabt-tools (can be installed via wapm).  Addtionally, consider passing the `-C opt-level=z` flag to `rustc` to optimize for size.
