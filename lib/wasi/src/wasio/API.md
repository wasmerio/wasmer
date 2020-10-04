# WASIO API

## Basic

### wasio_wait

**Description**: Waits for the next event, returning the error code and user context associated with the event.

**Arguments**:

- `error_out: *mut __wasi_errno_t`: Pointer to the memory location to store the error code.
- `user_context: *mut UserContext`: Pointer to the memory location to store the user context.

### wasio_cancel

**Description**: Cancels an ongoing asynchronous operation.

**Arguments**:

- `token: CancellationToken`: The cancellation token associated with the operation to cancel.

**Return value**: The result of cancelling the operation. `0` when success, or `__WASI_EINVAL` when the given cancellation token is invalid.

## Timer

### wasio_delay

**Description**: Delays for the given nanoseconds.

**Arguments**:

- `nanoseconds: u64`: Number of nanoseconds to delay.
- `user_context: UserContext`: User context to associate with the completion event.
- `cancellation_token: *mut CancellationToken`: Pointer to the memory location to store the cancellation token.

**Returns**: 0 when success. the error code otherwise.

## Networking

### wasio_socket_create

**Descrption**: Creates a socket, allocating a file desciptor for it.

**Arguments**:

- `fd_out: u32`: Pointer to the memory location to store the new file descriptor.
- `domain: u32`: Socket domain: IPv4 or IPv6.
- `ty: u32`: Socket type: TCP, UDP or something else.
- `protocol: u32`: Socket protocol. Usually 0.

### wasio_socket_bind

**Description**: Binds a socket to an address.

**Arguments**:

- `fd`: File descriptor to the socket to operate on.
- `sockaddr`: Pointer to socket address to bind.
- `sockaddr_size`: Size in bytes of the socket address.
