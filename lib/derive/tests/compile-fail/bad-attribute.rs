extern crate wasmer;

use wasmer::{LazyInit, WasmerEnv, Memory};

#[derive(WasmerEnv)]
struct BadAttribute {
    #[wasmer(extraport)] //~ Unexpected identifier `extraport`. Expected `export`.
    memory: LazyInit<Memory>,
}

fn main() {}