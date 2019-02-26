use libc::execvp;
use std::ffi::CString;
use wasmer_runtime_core::vm::Ctx;

pub fn _execvp(ctx: &mut Ctx, command_name_offset: u32, argv_offset: u32) -> i32 {
    use std::cell::Cell;

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

    let args_vec_of_c_string_pointers: Vec<*const Cell<u8>> = emscripten_memory.view()
        [((argv_offset / 4) as usize)..]
        .iter()
        .map(|cell: &Cell<u32>| cell.get())
        .take_while(|&byte| byte != 0)
        .map(|offset| (emscripten_memory.view::<u8>()[(offset as usize)..]).as_ptr())
        .collect();
    let args_pointer = args_vec_of_c_string_pointers.as_ptr() as *const *const i8;
    unsafe { execvp(command_name_string.as_ptr() as _, args_pointer) }
}
