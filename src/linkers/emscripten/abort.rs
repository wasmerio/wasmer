use crate::webassembly::Instance;
use std::process;

pub extern "C" fn abort(_code: i32, _instance: &Instance) {
    process::abort();
    // abort!("Aborted")
}
