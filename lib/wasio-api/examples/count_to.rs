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
