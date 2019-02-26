use libc::execvp;
use std::ffi::CString;
use wasmer_runtime_core::vm::Ctx;
use std::slice;
use std::cell::Cell;

pub fn _execvp(ctx: &mut Ctx, command_name_offset: u32, argv_offset: u32) -> i32 {
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
    let mut yy: Vec<*const i8> = emscripten_memory.view()[((argv_offset / 4) as usize)..]
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

    // push a nullptr on to the end of the args array, cuz C is terrible
    yy.push(std::ptr::null());

    // construct raw pointers and hand them to `execvp`
    let command_pointer = command_name_string.as_ptr() as *const i8;
    let args_pointer = yy.as_ptr();
    unsafe { execvp(command_pointer, args_pointer) }
}
