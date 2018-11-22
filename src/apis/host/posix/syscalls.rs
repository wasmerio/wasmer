// NOTE: These syscalls only support wasm_32 for now because they take u32 offset

// use libc::{
//     c_int,
//     c_void,
//     size_t,
//     ssize_t,
//     exit,
//     read,
//     write,
//     open,
//     close,
// };

// use crate::webassembly::{Instance};

// /// emscripten: ___syscall1
// pub extern "C" fn sys_exit(status: c_int, _instance: &mut Instance) {
//     debug!("host::sys_exit");
//     unsafe { exit(status); }
// }

// /// emscripten: ___syscall3
// pub extern "C" fn sys_read(fd: c_int, buf: *mut c_void, count: size_t, instance: &mut Instance) -> ssize_t {
//     debug!("host::sys_read");
//     let buf_addr = instance.memory_offset_addr(0, buf as usize) as *mut c_void;
//     unsafe { read(fd, buf_addr, count) }
// }

// /// emscripten: ___syscall4
// pub extern "C" fn sys_write(which: c_int, mode: c_int, instance: &mut Instance) -> c_int {
//     debug!("host::sys_write({}, {})", which, mode);
//     // unsafe { write(which, mode) };
//     0
// }
// /// emscripten: ___syscall5
// pub extern "C" fn sys_open(path: u32, flags: c_int, mode: c_int, instance: &mut Instance) -> c_int {
//     debug!("host::sys_open({}, {}, {})", path, flags, mode);
//     // let path_addr = instance.memory_offset_addr(0, path as usize) as *const i8;
//     // unsafe { open(path_addr, flags, mode) };
//     -2
// }

// /// emscripten: ___syscall6
// pub extern "C" fn sys_close(fd: c_int, _instance: &mut Instance) -> c_int {
//     debug!("host::sys_close");
//     unsafe { close(fd) }
// }
