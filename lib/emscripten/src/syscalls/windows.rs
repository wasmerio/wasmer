use libc::{c_int, c_uint, size_t, write};

use crate::varargs::VarArgs;
use wasmer_runtime_core::vm::Ctx;


/// write
pub fn ___syscall4(which: c_int, mut varargs: VarArgs, ctx: &mut Ctx) -> c_int {
    debug!("emscripten::___syscall4 (write) {}", which);
    let fd: i32 = varargs.get(ctx);
    let buf: u32 = varargs.get(ctx);
    let count: c_uint = varargs.get(ctx);
    debug!("=> fd: {}, buf: {}, count: {}", fd, buf, count);
    let buf_addr = emscripten_memory_pointer!(ctx.memory(0), buf) as *const c_void;
    unsafe { write(fd, buf_addr, count as _) as i32 }
}
