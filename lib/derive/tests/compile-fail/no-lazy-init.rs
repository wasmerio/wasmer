extern crate wasmer;

use wasmer::{LazyInit, WasmerEnv, Memory};

#[derive(WasmerEnv)]
struct ExportNotWrappedInLazyInit {
    #[wasmer(export)] //~ WasmerEnv derive expects all `exports` to be wrapped in `LazyInit`
    memory: Memory,
}

fn main() {}