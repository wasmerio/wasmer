#![no_main]
#[macro_use] extern crate libfuzzer_sys;
extern crate wasmer_runtime;

use wasmer_runtime::{
    instantiate,
    imports,
};

fuzz_target!(|data: &[u8]| {
    let import_object = imports! {};
    instantiate(data, &import_object);
});
