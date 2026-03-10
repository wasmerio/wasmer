#![feature(wasi_ext)]

use kernel_net::{schedule, Epoll, Tcp4Listener, TcpStream};
use std::sync::Arc;

fn serve(stream: Arc<TcpStream>, mut all: Vec<u8>) {
    let stream2 = stream.clone();
    stream.read_async(
        Vec::with_capacity(512),
        move |result| {
            match result {
                Ok(buf) => {
                    if buf.len() == 0 {
                        return;
                    }
                    all.extend_from_slice(&buf);
                    if all.len() > 4096 {
                        println!("header too large");
                        return;
                    }
                    let s = match ::std::str::from_utf8(&all) {
                        Ok(x) => x,
                        Err(e) => {
                            println!("not utf8: {:?}", e);
                            return;
                        }
                    };
                    if let Some(_pos) = s.find("\r\n\r\n") {
                        let body = format!(
                            "Hello, world!\n",
                        ).into_bytes();
                        let stream = stream2.clone();
                        stream2.write_all_async(
                            format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
                                body.len()
                            ).into_bytes(),
                            |_| {
                                stream.write_all_async(body, |_| {});
                            }
                        );
                    } else {
                        schedule(|| serve(stream2, all));
                    }
                }
                Err(code) => {
                    println!("failed to read; code = {}", code);
                }
            }
        }
    );
}

fn main() {
    let epoll = Arc::new(Epoll::new());
    let listener = Arc::new(Tcp4Listener::new("0.0.0.0", 2011, 128).unwrap());

    listener.accept_async(epoll.clone(), |stream| match stream {
        Ok(stream) => {
            serve(stream, vec![]);
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
