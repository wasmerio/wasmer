use std::time::Duration;
use wasio::task::Task;
use wasio::thread::delay;

fn main() {
    Task::spawn(Box::pin(root_task()));
    wasio::executor::enter();
}

async fn root_task() {
    println!("Spawning workers");
    for i in 0..16 {
        Task::spawn(Box::pin(worker_task(i)));
    }
}

async fn worker_task(id: i32) {
    println!("Worker {} started", id);
    let mut i: i32 = 0;
    loop {
        println!("Hello ({}:{})", id, i);
        i += 1;
        delay(Duration::from_millis(1000)).await;
    }
}
