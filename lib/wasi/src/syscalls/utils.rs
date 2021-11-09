#![allow(unused, clippy::too_many_arguments, clippy::cognitive_complexity)]

use super::types::*;
use crate::{
    ptr::{Array, WasmPtr},
    state::{
        self, fs_error_into_wasi_err, iterate_poll_events, poll,
        virtual_file_type_to_wasi_file_type, Fd, Inode, InodeVal, Kind, PollEvent,
        PollEventBuilder, WasiState, MAX_SYMLINKS,
    },
    WasiEnv, WasiError,
};
use std::borrow::Borrow;
use std::convert::{Infallible, TryInto};
use std::io::{self, Read, Seek, Write};
use tracing::{debug, trace};
use wasmer::{Memory, RuntimeError, Value, WasmCell};
use wasmer_vfs::{FsError, VirtualFile};

pub fn map_io_err(err: std::io::Error) -> __wasi_errno_t {
    use std::io::ErrorKind;
    match err.kind() {
        ErrorKind::NotFound => __WASI_ENOENT,
        ErrorKind::PermissionDenied => __WASI_EPERM,
        ErrorKind::ConnectionRefused => __WASI_ECONNREFUSED,
        ErrorKind::ConnectionReset => __WASI_ECONNRESET,
        ErrorKind::ConnectionAborted => __WASI_ECONNABORTED,
        ErrorKind::NotConnected => __WASI_ENOTCONN,
        ErrorKind::AddrInUse => __WASI_EADDRINUSE,
        ErrorKind::AddrNotAvailable => __WASI_EADDRNOTAVAIL,
        ErrorKind::BrokenPipe => __WASI_EPIPE,
        ErrorKind::AlreadyExists => __WASI_EEXIST,
        ErrorKind::WouldBlock => __WASI_EAGAIN,
        ErrorKind::InvalidInput => __WASI_EIO,
        ErrorKind::InvalidData => __WASI_EIO,
        ErrorKind::TimedOut => __WASI_ETIMEDOUT,
        ErrorKind::WriteZero => __WASI_EIO,
        ErrorKind::Interrupted => __WASI_EINTR,
        ErrorKind::Other => __WASI_EIO,
        ErrorKind::UnexpectedEof => __WASI_EIO,
        ErrorKind::Unsupported => __WASI_ENOTSUP,
        _ => __WASI_EIO
    }
}

pub fn write_bytes_inner<T: Write>(
    mut write_loc: T,
    memory: &Memory,
    iovs_arr_cell: &[WasmCell<__wasi_ciovec_t>],
) -> Result<u32, __wasi_errno_t> {
    let mut bytes_written = 0;
    for iov in iovs_arr_cell {
        let iov_inner = iov.get();
        let bytes = WasmPtr::<u8, Array>::new(iov_inner.buf).deref(memory, 0, iov_inner.buf_len)?;
        write_loc
            .write_all(&bytes.iter().map(|b_cell| b_cell.get()).collect::<Vec<u8>>())
            .map_err(|_| __WASI_EIO)?;

        // TODO: handle failure more accurately
        bytes_written += iov_inner.buf_len;
    }
    Ok(bytes_written)
}

pub fn write_bytes<T: Write>(
    mut write_loc: T,
    memory: &Memory,
    iovs_arr_cell: &[WasmCell<__wasi_ciovec_t>],
) -> Result<u32, __wasi_errno_t> {
    let result = write_bytes_inner(&mut write_loc, memory, iovs_arr_cell);
    write_loc.flush();
    result
}

pub fn read_bytes<T: Read>(
    mut reader: T,
    memory: &Memory,
    iovs_arr_cell: &[WasmCell<__wasi_iovec_t>],
) -> Result<u32, __wasi_errno_t> {
    let mut bytes_read = 0;

    // We allocate the raw_bytes first once instead of
    // N times in the loop.
    let mut raw_bytes: Vec<u8> = vec![0; 1024];

    for iov in iovs_arr_cell {
        let iov_inner = iov.get();
        raw_bytes.clear();
        raw_bytes.resize(iov_inner.buf_len as usize, 0);
        bytes_read += reader.read(&mut raw_bytes).map_err(|_| __WASI_EIO)? as u32;
        unsafe {
            memory
                .uint8view()
                .subarray(
                    iov_inner.buf as u32,
                    iov_inner.buf as u32 + iov_inner.buf_len as u32,
                )
                .copy_from(&raw_bytes);
        }
    }
    Ok(bytes_read)
}

/// checks that `rights_check_set` is a subset of `rights_set`
pub fn has_rights(rights_set: __wasi_rights_t, rights_check_set: __wasi_rights_t) -> bool {
    rights_set | rights_check_set == rights_set
}

#[must_use]
pub fn write_buffer_array(
    memory: &Memory,
    from: &[Vec<u8>],
    ptr_buffer: WasmPtr<WasmPtr<u8, Array>, Array>,
    buffer: WasmPtr<u8, Array>,
) -> __wasi_errno_t {
    let ptrs = wasi_try!(ptr_buffer.deref(memory, 0, from.len() as u32));

    let mut current_buffer_offset = 0;
    for ((i, sub_buffer), ptr) in from.iter().enumerate().zip(ptrs.iter()) {
        debug!("ptr: {:?}, subbuffer: {:?}", ptr, sub_buffer);
        let new_ptr = WasmPtr::new(buffer.offset() + current_buffer_offset);
        ptr.set(new_ptr);

        let cells = wasi_try!(new_ptr.deref(memory, 0, sub_buffer.len() as u32 + 1));

        for (cell, &byte) in cells.iter().zip(sub_buffer.iter().chain([0].iter())) {
            cell.set(byte);
        }
        current_buffer_offset += sub_buffer.len() as u32 + 1;
    }

    __WASI_ESUCCESS
}