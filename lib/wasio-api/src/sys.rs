use crate::types::*;

#[link(wasm_import_module = "wasio_unstable")]
extern "C" {
    pub fn wait(
        error_out: *mut __wasi_errno_t,
        user_context_out: *mut UserContext,
    ) -> __wasi_errno_t;
    pub fn cancel(token: CancellationToken) -> __wasi_errno_t;

    pub fn delay(
        nanoseconds: u64,
        user_context: UserContext,
        ct_out: *mut CancellationToken,
    ) -> __wasi_errno_t;
    pub fn async_nop(
        user_context: UserContext,
        ct_out: *mut CancellationToken,
    ) -> __wasi_errno_t;

    pub fn socket_create(
        fd_out: *mut __wasi_fd_t,
        domain: __wasio_socket_domain_t,
        ty: __wasio_socket_type_t,
        protocol: __wasio_socket_protocol_t,
    ) -> __wasi_errno_t;
    pub fn socket_bind(
        fd: __wasi_fd_t,
        sockaddr: *const u8,
        sockaddr_size: u32,
    ) -> __wasi_errno_t;
    pub fn socket_listen(fd: __wasi_fd_t) -> __wasi_errno_t;
    pub fn socket_pre_accept(
        fd: __wasi_fd_t,
        user_context: UserContext,
        ct_out: *mut CancellationToken,
    ) -> __wasi_errno_t;
    pub fn socket_accept(fd_out: *mut __wasi_fd_t) -> __wasi_errno_t;
    pub fn write(
        fd: __wasi_fd_t,
        si_data: *const __wasi_ciovec_t,
        si_data_len: u32,
        si_flags: __wasi_siflags_t,
        so_datalen: *mut u32,
        user_context: UserContext,
        ct_out: *mut CancellationToken,
    ) -> __wasi_errno_t;
    pub fn read(
        fd: __wasi_fd_t,
        ri_data: *const __wasi_ciovec_t,
        ri_data_len: u32,
        ri_flags: __wasi_riflags_t,
        ro_datalen: *mut u32,
        ro_flags: *mut __wasi_roflags_t,
        user_context: UserContext,
        ct_out: *mut CancellationToken,
    ) -> __wasi_errno_t;
}
