use crate::macros::emscripten_memory_ptr;
use crate::syscalls::emscripten_vfs::{EmscriptenVfs, VirtualFd};
use crate::varargs::VarArgs;
use crate::EmscriptenData;
use wasmer_runtime_core::vm::Ctx;

fn translate_to_host_file_descriptors(
    vfs: &EmscriptenVfs,
    set_ptr: *mut libc::fd_set,
    nfds: i32,
) -> (i32, Vec<i32>) {
    let pairs = (0..nfds)
        .map(|vfd| (vfd, vfs.get_host_socket_fd(&VirtualFd(vfd)).unwrap_or(-1)))
        .filter(|(vfd, _)| unsafe { libc::FD_ISSET(*vfd, set_ptr) })
        .collect::<Vec<_>>();
    let max = pairs
        .iter()
        .map(|(_, host_fd)| *host_fd)
        .max()
        .unwrap_or(-1)
        + 1;
    let mut internal_handles = vec![0; max as usize];
    unsafe { libc::FD_ZERO(set_ptr) };
    pairs.iter().for_each(|(vfd, host_fd)| {
        internal_handles[*host_fd as usize] = *vfd;
        unsafe {
            libc::FD_SET(*host_fd, set_ptr);
        };
    });
    (max, internal_handles)
}

fn translate_to_virtual_file_descriptors(set_ptr: *mut libc::fd_set, internal_handles: Vec<i32>) {
    let virtual_fds = internal_handles
        .iter()
        .enumerate()
        .filter(|(host_fd, _)| unsafe { libc::FD_ISSET(*host_fd as i32, set_ptr) })
        .map(|(_, vfd)| *vfd)
        .collect::<Vec<_>>();
    unsafe { libc::FD_ZERO(set_ptr) };
    virtual_fds
        .iter()
        .for_each(|vfd| unsafe { libc::FD_SET(*vfd, set_ptr) });
}

/// select
#[allow(clippy::cast_ptr_alignment)]
pub fn ___syscall142(ctx: &mut Ctx, _: libc::c_int, mut varargs: VarArgs) -> libc::c_int {
    debug!("emscripten::___syscall142 (select)");
    let nfds: i32 = varargs.get(ctx);
    let readfds: u32 = varargs.get(ctx);
    let writefds: u32 = varargs.get(ctx);
    let _exceptfds: u32 = varargs.get(ctx);
    let _timeout: i32 = varargs.get(ctx);
    assert!(nfds <= 64, "`nfds` must be less than or equal to 64");
    let emscripten_memory = ctx.memory(0);
    let read_set_ptr = emscripten_memory_ptr(emscripten_memory, readfds) as _;
    let write_set_ptr = emscripten_memory_ptr(emscripten_memory, writefds) as _;
    let vfs = unsafe { (*(ctx.data as *const EmscriptenData)).vfs.as_ref().unwrap() };
    let (read_host_nfds, read_lookup) = translate_to_host_file_descriptors(vfs, read_set_ptr, nfds);
    let (write_host_nfds, write_lookup) =
        translate_to_host_file_descriptors(vfs, write_set_ptr, nfds);
    let host_nfds = std::cmp::max(read_host_nfds, write_host_nfds);
    // TODO: timeout and except fds set
    let result = unsafe { libc::select(host_nfds, read_set_ptr, write_set_ptr, 0 as _, 0 as _) };
    translate_to_virtual_file_descriptors(read_set_ptr, read_lookup);
    translate_to_virtual_file_descriptors(write_set_ptr, write_lookup);
    debug!("select returns {}", result);
    result
}
