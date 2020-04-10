//! A kind of meta-build.rs that can be configured to do different things.
//!
//! Please try to keep this file as clean as possible.

use generate_emscripten_tests;
use generate_wasi_tests;

fn main() {
    generate_wasi_tests::build();
    generate_emscripten_tests::build();
}
