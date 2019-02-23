use wasmer_runtime_core::vm::Ctx;
use libc::execvp;
use crate::utils::read_string_from_wasm;
use wasmer_runtime_core::memory::Memory;
use std::ffi::CString;

pub fn _execvp(ctx: &mut Ctx, command_name_offset: u32, argv_offset: u32) -> i32 {
    use std::cell::Cell;
    fn get_raw_ptr(memory: &Memory, offset: u32) -> *const Cell<u8> {
        (memory.view::<u8>()[(offset as usize)..]).as_ptr()
    }

    fn read_c_string_from_wasm(memory: &Memory, offset: u32) -> CString {
        let v: Vec<u8> = memory.view()[(offset as usize)..]
            .iter()
            .map(|cell| cell.get())
            .take_while(|&byte| byte != 0)
            .collect();
        CString::new(v).unwrap()
    }

    // a single reference to re-use
    let emscripten_memory = ctx.memory(0);

    let command_name_ptr = get_raw_ptr(&emscripten_memory, command_name_offset);
    let argv_ptr = get_raw_ptr(&emscripten_memory, argv_offset);

    // read command name as string
    let command_name_string = read_c_string_from_wasm(&emscripten_memory, command_name_offset);

    // the accumulated array of args
    let mut args: Vec<CString> = vec![];
    // an offset binding that we move forward along the argv array
    let mut offset = argv_offset;
    // the pointer to the raw data behind the offset
    let mut ptr = get_raw_ptr(&emscripten_memory, offset);

    unsafe {
        // while not at the end of this C-style array,
        while ptr != std::ptr::null() {
            // increment to next pointer
            let arg = read_c_string_from_wasm(&emscripten_memory, offset);
            args.push(arg);
            // update the offset and the raw pointer for the next iteration
            offset = offset + 1;
            ptr = get_raw_ptr(&emscripten_memory, offset);
        }
    }

    // convert the vec of CStrings to the array of arrays - yikes
    let args_ptr = args.as_ptr() as *const *const libc::c_char;

    unsafe {
        execvp(command_name_string.as_ptr() as _, args_ptr)
    }
}
