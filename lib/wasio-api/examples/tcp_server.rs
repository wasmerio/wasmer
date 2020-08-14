use std::time::Duration;
use wasio::io;
use wasio::net;
use wasio::task::Task;
use wasio::thread::delay;
use wasio::types::*;

fn main() {
    Task::spawn(Box::pin(root_task()));
    wasio::executor::enter();
}

async fn root_task() {
    println!("Creating socket");
    let fd = net::socket(AF_INET, SOCK_STREAM, 0).unwrap();
    println!("fd = {}", fd);

    net::bind4(
        fd,
        &SockaddrIn {
            sin_family: AF_INET as _,
            sin_port: 9000u16.to_be(),
            sin_addr: [0u8; 4],
            sin_zero: [0u8; 8],
        },
    )
    .unwrap();
    println!("Binded.");

    net::listen(fd).unwrap();
    println!("Listen started.");

    loop {
        let conn = net::accept(fd).await.unwrap();
        println!("New connection.");
        Task::spawn(Box::pin(conn_worker(conn)));
    }
}

async fn conn_worker(conn: __wasi_fd_t) {
    for i in 0..10 {
        let s = format!("Hello from Wasio! (#{})\n", i);
        match io::write(conn, s.as_bytes()).await {
            Ok(n) => {
                delay(Duration::from_millis(1000)).await;
            }
            Err(e) => {
                println!("Connection error: {:?}", e);
                break;
            }
        }
    }
    net::close(conn);
}
