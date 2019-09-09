#![no_main]
#[macro_use]
extern crate libfuzzer_sys;
extern crate wasmer_runtime;

use wasmer_runtime::compile;

fuzz_target!(|data: &[u8]| {
    let _ = compile(data);
});
