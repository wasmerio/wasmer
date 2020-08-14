# `wasio` [![Build Status](https://img.shields.io/azure-devops/build/wasmerio/wasmer/3.svg?style=flat-square)](https://dev.azure.com/wasmerio/wasmer/_build/latest?definitionId=3&branchName=master) [![Join Wasmer Slack](https://img.shields.io/static/v1?label=Slack&message=join%20chat&color=brighgreen&style=flat-square)](https://slack.wasmer.io) [![MIT License](https://img.shields.io/github/license/wasmerio/wasmer.svg?style=flat-square)](https://github.com/wasmerio/wasmer/blob/master/LICENSE)

Wasio allows using `async`/`await` syntax and while also adding
networking capabilities, so you can use them when compiling to WebAssembly.

This crate enables it's easy usage within Rust.

## Example Implementation

Count to 10 script:

```rust
use std::time::{Duration, SystemTime};
use wasio::task::Task;
use wasio::thread::delay;

fn main() {
    Task::spawn(Box::pin(root_task()));
    wasio::executor::enter();
}

async fn root_task() {
    const N: usize = 10;
    println!("Counting to {}:", N);
    for i in 0..N {
        delay(Duration::from_millis(1000)).await;
        println!("* {}", i + 1);
    }
    std::process::exit(0);
}
```
