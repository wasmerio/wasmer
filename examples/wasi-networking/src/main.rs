#![feature(wasi_ext)]

use kernel_net::{schedule, Epoll, Tcp4Listener, TcpStream};
use std::sync::Arc;

fn do_echo(stream: Arc<TcpStream>, buf: Vec<u8>) {
    let stream2 = stream.clone();
    stream.read_async(buf, move |result| match result {
        Ok(buf) => {
            if buf.len() == 0 {
                return;
            }
            let stream = stream2.clone();
            stream2.write_all_async(buf, move |result| match result {
                Ok(buf) => {
                    schedule(|| {
                        do_echo(stream, buf);
                    });
                }
                Err(code) => {
                    println!("failed to write; code = {}", code);
                }
            });
        }
        Err(code) => {
            println!("failed to read; code = {}", code);
        }
    });
}

fn main() {
    let epoll = Arc::new(Epoll::new());
    let listener = Arc::new(Tcp4Listener::new("0.0.0.0", 2001, 128).unwrap());

    listener.accept_async(epoll.clone(), |stream| match stream {
        Ok(stream) => {
            do_echo(stream, Vec::with_capacity(16384));
            Ok(())
        }
        Err(code) => {
            println!("failed to accept; code = {}", code);
            Err(())
        }
    });
    println!("start epoll");
    unsafe {
        epoll.run();
    }
}
