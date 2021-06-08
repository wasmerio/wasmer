use wasmer_wasi_types::*;

#[link(wasm_import_module = "wasi_experimental_network_unstable")]
extern "C" {
    pub fn sock_create(fd_out: *mut __wasi_fd_t) -> __wasi_errno_t;
    pub fn sock_bind();
    pub fn sock_connect();
    pub fn sock_listen();
    pub fn sock_accept();
    pub fn sock_send();
    pub fn sock_recv();
    pub fn sock_shutdown();
}
