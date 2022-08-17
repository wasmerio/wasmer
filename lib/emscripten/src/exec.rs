use crate::varargs::VarArgs;
use crate::EmEnv;
use libc::c_char;
use libc::execvp as libc_execvp;
use std::ffi::CString;
use wasmer::{FunctionEnvMut, WasmPtr};

pub fn execvp(ctx: FunctionEnvMut<EmEnv>, command_name_offset: u32, argv_offset: u32) -> i32 {
    // a single reference to re-use
    let memory = ctx.data().memory(0);
    let emscripten_memory = memory.view(&ctx);

    // read command name as string
    let command_name_string_vec = WasmPtr::<u8>::new(command_name_offset)
        .read_until(&emscripten_memory, |&byte| byte == 0)
        .unwrap();
    let command_name_string = CString::new(command_name_string_vec).unwrap();

    // get the array of args
    let argv = WasmPtr::<WasmPtr<u8>>::new(argv_offset)
        .read_until(&emscripten_memory, |&ptr| ptr.is_null())
        .unwrap();
    let arg_strings: Vec<CString> = argv
        .into_iter()
        .map(|ptr| {
            let vec = ptr
                .read_until(&emscripten_memory, |&byte| byte == 0)
                .unwrap();
            CString::new(vec).unwrap()
        })
        .collect();
    let mut argv: Vec<*const c_char> = arg_strings.iter().map(|s| s.as_ptr()).collect();

    // push a nullptr on to the end of the args array
    argv.push(std::ptr::null());

    // construct raw pointers and hand them to `execvp`
    let command_pointer = command_name_string.as_ptr() as *const i8;
    let args_pointer = argv.as_ptr();
    unsafe { libc_execvp(command_pointer as *const _, args_pointer as *const *const _) }
}

/// execl
pub fn execl(
    _ctx: FunctionEnvMut<EmEnv>,
    _path_ptr: i32,
    _arg0_ptr: i32,
    _varargs: VarArgs,
) -> i32 {
    debug!("emscripten::execl");
    -1
}

/// execle
pub fn execle(
    _ctx: FunctionEnvMut<EmEnv>,
    _path_ptr: i32,
    _arg0_ptr: i32,
    _varargs: VarArgs,
) -> i32 {
    debug!("emscripten::execle");
    -1
}
