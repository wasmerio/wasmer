use crate::macros::emscripten_memory_ptr;
use crate::syscalls::emscripten_vfs::{FileHandle, VirtualFd};
use crate::varargs::VarArgs;
use std::collections::HashMap;
use std::ffi::c_void;
use std::slice;
use wasmer_runtime_core::vm::Ctx;

#[cfg(feature = "vfs")]
#[derive(Debug)]
struct FdPair {
    pub virtual_fd: i32,
    pub host_fd: i32,
}

#[cfg(feature = "vfs")]
fn translate_to_host_file_descriptors(
    ctx: &mut Ctx,
    mut varargs: &mut VarArgs,
    nfds: i32,
    fds_set_offset: u32,
) -> (i32, HashMap<i32, i32>, Vec<FdPair>) {
    let set_ptr = emscripten_memory_ptr(ctx.memory(0), fds_set_offset) as *mut _; // e.g. libc::unix::bsd::fd_set
    let set_u8_ptr = set_ptr as *mut u8;

    let bit_array_size = if nfds >= 0 { (nfds + 7) / 8 } else { 0 } as usize;
    let end_offset = fds_set_offset as usize + bit_array_size;
    let set_view = &ctx.memory(0).view::<u8>()[(fds_set_offset as usize)..end_offset];
    use bit_field::BitArray;
    //    let check = set_ptr.get_bit(1);

    let fds_slice = unsafe { slice::from_raw_parts(set_u8_ptr, bit_array_size) };

    //    (0usize..nfds as usize).filter_map(|x| fds_slice.get_bit(x));
    //    let ofds = (0..nfds).filter_map(|v| fd_slice.v)
    let original_fds: Vec<i32> = (0..nfds)
        .filter_map(|virtual_fd| {
            if fds_slice.get_bit(virtual_fd as usize) {
                Some(virtual_fd as i32)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    let vfs = crate::env::get_emscripten_data(ctx).vfs.as_mut().unwrap();

    // virtual read and write file descriptors
    let file_descriptor_pairs = original_fds
        .iter()
        .filter(|vfd| {
            if let FileHandle::VirtualFile(handle) = vfs.fd_map.get(&VirtualFd(**vfd)).unwrap() {
                debug!(
                    "skipping virtual fd {} (vbox handle {}) because is a virtual file",
                    *vfd, *handle
                );
                false
            } else {
                true
            }
        })
        .map(|vfd| {
            let vfd = VirtualFd(*vfd);
            let file_handle = vfs.fd_map.get(&vfd).unwrap();
            let host_fd = match file_handle {
                FileHandle::Socket(host_fd) => host_fd,
                //                FileHandle::VirtualFile(handle) => handle,
                _ => panic!(),
            };
            let pair = FdPair {
                virtual_fd: vfd.0,
                host_fd: *host_fd,
            };
            // swap the read descriptors
            unsafe {
                libc::FD_CLR(pair.virtual_fd, set_ptr);
                libc::FD_SET(pair.host_fd, set_ptr);
            };
            pair
        })
        .collect::<Vec<_>>();

    let mut sz = 0;

    // helper look up tables
    let mut lookup = HashMap::new();
    for pair in file_descriptor_pairs.iter() {
        //        if pair.virtual_fd > sz { sz = pair.host_fd }
        if pair.host_fd > sz {
            sz = pair.host_fd
        }
        lookup.insert(pair.host_fd, pair.virtual_fd);
    }

    let max_file_descriptor = sz;
    (max_file_descriptor, lookup, file_descriptor_pairs)
}

#[cfg(feature = "vfs")]
fn translate_to_virtual_file_descriptors(
    ctx: &mut Ctx,
    nfds: i32,
    fds_set_offset: u32,
    lookup: HashMap<i32, i32>,
) -> Vec<FdPair> {
    let set_ptr = emscripten_memory_pointer!(ctx.memory(0), fds_set_offset) as *mut _;
    let set_u8_ptr = set_ptr as *mut u8;
    let fds_slice = unsafe { slice::from_raw_parts_mut(set_u8_ptr, nfds as usize) };
    use bit_field::BitArray;

    let fds = (0..nfds)
        .filter_map(|virtual_fd| {
            if fds_slice.get_bit(virtual_fd as usize) {
                Some(virtual_fd as i32)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    // swap descriptors back
    let pairs = fds
        .iter()
        .filter_map(|host_fd| {
            lookup
                .get(&host_fd)
                .map(|virtual_fd| (*virtual_fd, host_fd))
        })
        .map(|(virtual_fd, host_fd)| {
            unsafe {
                libc::FD_CLR(*host_fd, set_ptr);
                libc::FD_SET(virtual_fd, set_ptr);
            }
            FdPair {
                virtual_fd,
                host_fd: *host_fd,
            }
        })
        .collect::<Vec<_>>();
    pairs
}

/// select
#[cfg(feature = "vfs")]
#[allow(clippy::cast_ptr_alignment)]
pub fn ___syscall142(ctx: &mut Ctx, _which: libc::c_int, mut varargs: VarArgs) -> libc::c_int {
    debug!("emscripten::___syscall142 (newselect) {}", _which);
    let nfds: i32 = varargs.get(ctx);
    let readfds: u32 = varargs.get(ctx);
    let writefds: u32 = varargs.get(ctx);
    let _exceptfds: u32 = varargs.get(ctx);
    let timeout: i32 = varargs.get(ctx);
    assert!(nfds <= 64, "`nfds` must be less than or equal to 64");
    let readfds_set_ptr = emscripten_memory_pointer!(ctx.memory(0), readfds) as *mut _;
    let writefds_set_ptr = emscripten_memory_pointer!(ctx.memory(0), writefds) as *mut _;

    //    debug!(" select read descriptors: {:?}", read_fds);
    //
    //    debug!("select write descriptors: {:?}", write_fds);

    let (read_max, read_lookup, read_pairs) =
        translate_to_host_file_descriptors(ctx, &mut varargs, nfds, readfds);
    let (write_max, write_lookup, write_pairs) =
        translate_to_host_file_descriptors(ctx, &mut varargs, nfds, writefds);

    let max = if read_max > write_max {
        read_max
    } else {
        write_max
    };
    debug!("max host fd for select: {}", max);

    let mut sz = max;

    debug!(
        "set read descriptors BEFORE select: {:?}",
        read_pairs //            .iter()
                   //            .map(|pair| pair.virtual_fd)
                   //            .collect::<Vec<_>>()
    );
    debug!(
        "set write descriptors BEFORE select: {:?}",
        write_pairs //            .iter()
                    //            .map(|pair| pair.virtual_fd)
                    //            .collect::<Vec<_>>()
    );

    // call `select`
    sz = sz + 1;
    debug!(
        "readfds_set_ptr: {:?}",
        read_pairs
            .iter()
            .map(|pair| pair.host_fd)
            .collect::<Vec<_>>()
    );
    let fds_slice = unsafe { slice::from_raw_parts(readfds_set_ptr as *const u8, 4) } as &[u8];
    debug!("host read set before: {:?}", fds_slice);
    let mut result = unsafe { libc::select(sz, readfds_set_ptr, writefds_set_ptr, 0 as _, 0 as _) };
    debug!("host read set after: {:?}", fds_slice);

    unsafe {
        use libc::FD_ISSET;
        let s = 3;
        let x = [
            FD_ISSET(s, readfds_set_ptr),
            FD_ISSET(s + 1, readfds_set_ptr),
            FD_ISSET(s + 2, readfds_set_ptr),
            FD_ISSET(s + 3, readfds_set_ptr),
            FD_ISSET(s + 4, readfds_set_ptr),
            FD_ISSET(s + 5, readfds_set_ptr),
            FD_ISSET(s + 6, readfds_set_ptr),
            FD_ISSET(s + 7, readfds_set_ptr),
            FD_ISSET(s + 8, readfds_set_ptr),
            FD_ISSET(s + 9, readfds_set_ptr),
            FD_ISSET(s + 10, readfds_set_ptr),
            FD_ISSET(s + 11, readfds_set_ptr),
        ];
        debug!("sets (start with fd #{}: {:?}", s, x);
    }

    if result == -1 {
        panic!(
            "result returned from select was -1. The errno code: {}",
            errno::errno()
        );
    }

    let read_pairs = translate_to_virtual_file_descriptors(ctx, sz, readfds, read_lookup);
    debug!(
        "select read descriptors after select completes: {:?}",
        read_pairs //            .iter()
                   //            .map(|pair| pair.virtual_fd)
                   //            .collect::<Vec<_>>()
    );

    let write_pairs = translate_to_virtual_file_descriptors(ctx, sz, writefds, write_lookup);
    debug!(
        "select write descriptors after select completes: {:?}",
        write_pairs //            .iter()
                    //            .map(|pair| pair.virtual_fd)
                    //            .collect::<Vec<_>>()
    );

    debug!("select returns {}", result);
    result
}
