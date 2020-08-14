use std::time::{Duration, SystemTime};
use wasio::task::Task;

fn main() {
    Task::spawn(Box::pin(root_task()));
    wasio::executor::enter();
}

async fn root_task() {
    const N: usize = 10000000;
    println!("Benchmarking");
    let start = SystemTime::now();
    for _ in 0..N {
        wasio::thread::async_nop().await;
    }
    let end = SystemTime::now();
    println!("Done. Time = {:?}", end.duration_since(start).unwrap());
    std::process::exit(0);
}
