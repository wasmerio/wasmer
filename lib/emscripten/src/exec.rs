use crate::varargs::VarArgs;
use crate::EmEnv;
use libc::execvp as libc_execvp;
use std::cell::Cell;
use std::ffi::CString;

pub fn execvp(ctx: &EmEnv, command_name_offset: u32, argv_offset: u32) -> i32 {
    // a single reference to re-use
    let emscripten_memory = ctx.memory(0);

    // read command name as string
    let command_name_string_vec: Vec<u8> = emscripten_memory.view()
        [(command_name_offset as usize)..]
        .iter()
        .map(|cell| cell.get())
        .take_while(|&byte| byte != 0)
        .collect();
    let command_name_string = CString::new(command_name_string_vec).unwrap();

    // get the array of args
    let mut argv: Vec<*const i8> = emscripten_memory.view()[((argv_offset / 4) as usize)..]
        .iter()
        .map(|cell: &Cell<u32>| cell.get())
        .take_while(|&byte| byte != 0)
        .map(|offset| {
            let p: *const i8 = (emscripten_memory.view::<u8>()[(offset as usize)..])
                .iter()
                .map(|cell| cell.as_ptr() as *const i8)
                .collect::<Vec<*const i8>>()[0];
            p
        })
        .collect();

    // push a nullptr on to the end of the args array
    argv.push(std::ptr::null());

    // construct raw pointers and hand them to `execvp`
    let command_pointer = command_name_string.as_ptr() as *const i8;
    let args_pointer = argv.as_ptr();
    unsafe { libc_execvp(command_pointer as *const _, args_pointer as *const *const _) }
}

/// execl
pub fn execl(_ctx: &EmEnv, _path_ptr: i32, _arg0_ptr: i32, _varargs: VarArgs) -> i32 {
    debug!("emscripten::execl");
    -1
}

/// execle
pub fn execle(_ctx: &EmEnv, _path_ptr: i32, _arg0_ptr: i32, _varargs: VarArgs) -> i32 {
    debug!("emscripten::execle");
    -1
}
