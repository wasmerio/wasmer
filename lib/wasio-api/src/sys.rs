use crate::types::*;

#[link(wasm_import_module = "wasio_unstable")]
extern "C" {
    /// Waits for the next event, returning the error code and user context
    /// associated with the event.
    /// 
    /// The return value is always 0.
    /// 
    /// ## Arguments
    /// 
    /// - `error_out`: Pointer to the memory location to store the error code.
    /// - `user_context_out`: Pointer to the memory location to store the user context.
    pub fn wait(
        error_out: *mut __wasi_errno_t,
        user_context_out: *mut UserContext,
    ) -> __wasi_errno_t;

    /// Cancels an ongoing asynchronous operation synchronously.
    /// 
    /// Returns `0` on success, or `__WASI_EINVAL` when the given cancellation token is invalid.
    /// 
    /// ## Arguments
    /// 
    /// - `token`: The cancellation token associated with the operation to cancel.
    pub fn cancel(token: CancellationToken) -> __wasi_errno_t;

    /// Delays for the given nanoseconds.
    /// 
    /// ## Arguments
    /// 
    /// - `nanoseconds`: Number of nanoseconds to delay.
    /// - `user_context`: A 64-bit value to associate with the completion event.
    /// - `ct_out`: Pointer to the memory location to store the cancellation token.
    pub fn delay(
        nanoseconds: u64,
        user_context: UserContext,
        ct_out: *mut CancellationToken,
    ) -> __wasi_errno_t;

    pub fn async_nop(
        user_context: UserContext,
        ct_out: *mut CancellationToken,
    ) -> __wasi_errno_t;

    /// Creates a socket synchronously, allocating a file desciptor for it.
    /// 
    /// ## Arguments
    /// 
    /// - `fd_out`: Pointer to the memory location to store the new file descriptor.
    /// - `domain`: Socket domain: `AF_INET` for IPv4 or `AF_INET6` for IPv6.
    /// - `ty`: Socket type: SOCK_STREAM for TCP. UDP is not implemented yet.
    /// - `protocol`: Socket protocol. Currently always 0.
    pub fn socket_create(
        fd_out: *mut __wasi_fd_t,
        domain: __wasio_socket_domain_t,
        ty: __wasio_socket_type_t,
        protocol: __wasio_socket_protocol_t,
    ) -> __wasi_errno_t;

    /// Binds a socket to an address synchronously.
    /// 
    /// ## Arguments
    /// 
    /// - `fd`: The file descriptor to the socket to operate on.
    /// - `sockaddr`: Pointer to the socket address to bind to. See `SockaddrIn` and `SockaddrIn6`.
    /// - `sockaddr_size`: Size in bytes of the socket address. `size_of::<SockaddrIn>()` or `size_of::<SockaddrIn6>()`.
    pub fn socket_bind(
        fd: __wasi_fd_t,
        sockaddr: *const u8,
        sockaddr_size: u32,
    ) -> __wasi_errno_t;

    /// Connects a socket to a remote address.
    /// 
    /// ## Arguments
    /// 
    /// - `fd`: The file descriptor to the socket to operate on.
    /// - `sockaddr`: Pointer to the socket address to connect to. See `SockaddrIn` and `SockaddrIn6`.
    /// - `sockaddr_size`: Size in bytes of the socket address. `size_of::<SockaddrIn>()` or `size_of::<SockaddrIn6>()`.
    pub fn socket_connect(
        fd: __wasi_fd_t,
        sockaddr: *const u8,
        sockaddr_size: u32,
        user_context: UserContext,
        ct_out: *mut CancellationToken,
    ) -> __wasi_errno_t;

    /// Starts listening on a socket.
    /// 
    /// ## Arguments
    /// 
    /// - `fd`: The file descriptor to listen on.
    pub fn socket_listen(fd: __wasi_fd_t) -> __wasi_errno_t;

    /// Accepts a connection, storing it to an internal buffer.
    /// 
    /// ## Arguments
    /// 
    /// - `fd`: The file descriptor to accept on. Must be in listening state.
    /// - `user_context`: A 64-bit value to associate with the completion event.
    /// - `ct_out`: Pointer to the memory location to store the cancellation token.
    pub fn socket_pre_accept(
        fd: __wasi_fd_t,
        user_context: UserContext,
        ct_out: *mut CancellationToken,
    ) -> __wasi_errno_t;

    /// Fetches the accepted connection in the internal buffer and assignes a new file descriptor for it.
    /// 
    /// ## Arguments
    /// 
    /// - `fd_out`: Pointer to the memory location to store the new file descriptor.
    pub fn socket_accept(fd_out: *mut __wasi_fd_t) -> __wasi_errno_t;

    /// Sends data to a socket.
    /// 
    /// ## Arguments
    /// 
    /// - `fd`: The file descriptor to write to. Must be a connection.
    /// - `si_data`: An IO vector that contains the data to write.
    /// - `si_data_len`: Number of elements in `si_data`.
    /// - `si_flags`: Write flags.
    /// - `so_datalen`: Pointer to the memory location to store the number of bytes written.
    /// - `user_context`: A 64-bit value to associate with the completion event.
    /// - `ct_out`: Pointer to the memory location to store the cancellation token.
    /// 
    /// ## Lifetimes
    /// 
    /// WASIO buffers the IO vector `si_data` itself internally, but requires the buffers and `so_datalen`
    /// to be alive until the completion event is handled.
    pub fn socket_send(
        fd: __wasi_fd_t,
        si_data: *const __wasi_ciovec_t,
        si_data_len: u32,
        si_flags: __wasi_siflags_t,
        so_datalen: *mut u32,
        user_context: UserContext,
        ct_out: *mut CancellationToken,
    ) -> __wasi_errno_t;

    /// Receives data from a socket.
    /// 
    /// ## Arguments
    /// 
    /// - `fd`: The file descriptor to read from. Must be a connection.
    /// - `ri_data`: An IO vector that contains the buffers to read to.
    /// - `ri_data_len`: Number of elements in `ri_data`.
    /// - `ri_flags`: Read flags.
    /// - `ro_datalen`: Pointer to the memory location to store the number of bytes read.
    /// - `ro_flags`: Read flags. Currently unused.
    /// - `user_context`: A 64-bit value to associate with the completion event.
    /// - `ct_out`: Pointer to the memory location to store the cancellation token.
    /// 
    /// ## Lifetimes
    /// 
    /// WASIO buffers the IO vector `ri_data` itself internally, but requires the buffers and `ro_datalen`
    /// to be alive until the completion event is handled.
    pub fn socket_recv(
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
