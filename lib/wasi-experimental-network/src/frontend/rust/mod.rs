cfg_if::cfg_if! {
    if #[cfg(target_os = "wasi")] {
        mod c;
        mod socketaddr;
        mod tcp;

        pub use tcp::{TcpListener, TcpStream, Incoming};
    } else {
        pub use std::net::{TcpListener, TcpStream, Incoming};
    }
}
