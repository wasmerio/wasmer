//! Type definitions for WASIO.
#![allow(non_camel_case_types)]

use crate::{
    ptr::{Array, WasmPtr},
    syscalls::types::*,
    WasiFile, WasiFs,
};
use std::time::Duration;
use wasmer::{FromToNativeWasmType, Memory, ValueType};

/// The cancellation token of an ongoing asynchronous operation.
#[derive(Debug, Copy, Clone)]
#[repr(transparent)]
pub struct CancellationToken(pub u64);

unsafe impl ValueType for CancellationToken {}
unsafe impl FromToNativeWasmType for CancellationToken {
    type Native = i64;
    fn from_native(native: Self::Native) -> Self {
        CancellationToken(native as _)
    }
    fn to_native(self) -> Self::Native {
        self.0 as _
    }
}

/// An asynchronous oneshot operation.
pub enum AsyncOneshotOperation<'a> {
    /// Empty operation.
    Nop,

    /// Delays for a given duration.
    Delay(Duration),

    /// Writes data to an asynchronous file descriptor. No copies are made in userspace,
    /// so the (WebAssembly) caller must keep the memory region alive before
    /// this operation finishes.
    Write {
        memory: &'a Memory,
        fs: &'a mut WasiFs,
        fd: __wasi_fd_t,
        si_data: WasmPtr<__wasi_ciovec_t, Array>,
        si_data_len: u32,
        si_flags: __wasi_siflags_t,
        so_datalen: WasmPtr<u32>,
    },

    /// Reads data from an asynchronous file descriptor.
    Read {
        memory: &'a Memory,
        fs: &'a mut WasiFs,
        fd: __wasi_fd_t,
        ri_data: WasmPtr<__wasi_ciovec_t, Array>,
        ri_data_len: u32,
        ri_flags: __wasi_riflags_t,
        ro_datalen: WasmPtr<u32>,
        ro_flags: WasmPtr<__wasi_roflags_t>,
    },

    /// Accepts a connection on a socket.
    SocketPreAccept {
        fs: &'a mut WasiFs,
        fd: __wasi_fd_t, // fd
    },

    /// Connects a socket to a remote address.
    SocketConnect {
        memory: &'a Memory,
        fs: &'a mut WasiFs,
        fd: __wasi_fd_t,
        sockaddr_ptr: WasmPtr<u8, Array>,
        sockaddr_size: u32,
    },
}

/// An asynchronous stream operation.
pub enum AsyncStreamOperation {}

/// An synchronous operation.
pub enum SyncOperation<'a> {
    /// Cancels an ongoing asynchronous operation.
    Cancel(CancellationToken),

    /// Creates an asynchronous socket.
    SocketCreate(
        &'a Memory,
        WasmPtr<__wasi_fd_t>,
        &'a mut WasiFs,
        __wasio_socket_domain_t,
        __wasio_socket_type_t,
        __wasio_socket_protocol_t,
    ),

    /// Binds an asynchronous socket to an address.
    SocketBind(
        &'a Memory,
        &'a mut WasiFs,
        __wasi_fd_t,        // fd
        WasmPtr<u8, Array>, // sockaddr
        u32,                // sockaddr size
    ),

    /// Accepts a connection notified via a `SocketListen` stream.
    SocketAccept {
        memory: &'a Memory,
        fs: &'a mut WasiFs,
        fd_out: WasmPtr<__wasi_fd_t>, // fd
        sockaddr_ptr: WasmPtr<u8, Array>,
        sockaddr_size: u32,
    },

    /// Starts listening on a socket.
    SocketListen {
        fs: &'a mut WasiFs,
        fd: __wasi_fd_t, // fd
    },

    /// Gets the local or remote address of a socket.
    SocketAddr {
        memory: &'a Memory,
        fs: &'a mut WasiFs,
        fd: __wasi_fd_t, // fd
        sockaddr_ptr: WasmPtr<u8, Array>,
        sockaddr_size_ptr: WasmPtr<u32>,
        remote: bool,
    },
}

/// The user context that will be returned to WebAssembly, once a
/// requested asynchronous operation is completed.
#[derive(Debug, Copy, Clone)]
#[repr(transparent)]
pub struct UserContext(pub u64);

unsafe impl ValueType for UserContext {}
unsafe impl FromToNativeWasmType for UserContext {
    type Native = i64;
    fn from_native(native: Self::Native) -> Self {
        UserContext(native as _)
    }
    fn to_native(self) -> Self::Native {
        self.0 as _
    }
}

/// The `sockaddr_in` struct.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct SockaddrIn {
    pub sin_family: i16,
    pub sin_port: u16,
    pub sin_addr: [u8; 4],
    pub sin_zero: [u8; 8],
}

unsafe impl ValueType for SockaddrIn {}

/// The `sockaddr_in6` struct.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct SockaddrIn6 {
    pub sin6_family: i16,
    pub sin6_port: u16,
    pub sin6_flowinfo: u32,
    pub sin6_addr: [u8; 16],
    pub sin6_scope_id: u32,
}

unsafe impl ValueType for SockaddrIn6 {}

/// The type of a WASIO socket domain.
pub type __wasio_socket_domain_t = i32;

/// The type of a WASIO socket type.
pub type __wasio_socket_type_t = i32;

/// The type of a WASIO socket protocol.
pub type __wasio_socket_protocol_t = i32;
