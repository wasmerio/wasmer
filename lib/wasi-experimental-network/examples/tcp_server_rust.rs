use std::io::{Read, Write};
use wasmer_wasi_experimental_network::frontend::rust::*;

fn main() {
    println!("Creating, binding the socket + listening");

    let listener = TcpListener::bind("127.0.0.1:9002").unwrap();

    println!("Waiting to accept a new connection");

    for stream in listener.incoming() {
        let mut stream = stream.unwrap();

        let mut buffer = vec![0; 128];
        let number_of_bytes = stream.read(&mut buffer).unwrap();

        println!(
            "Read: `{:?}`",
            String::from_utf8_lossy(&buffer[..number_of_bytes])
        );

        stream.write(&buffer).unwrap();

        println!("Waiting to accept a new connection");
    }
}
