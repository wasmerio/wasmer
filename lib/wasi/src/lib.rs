#![deny(unused_imports, unused_variables, unused_unsafe, unreachable_patterns)]

#[macro_use]
extern crate log;
#[cfg(target = "windows")]
extern crate winapi;

#[macro_use]
mod macros;
mod ptr;
mod state;
mod syscalls;
mod utils;

use self::state::{WasiFs, WasiState};
use self::syscalls::*;

use std::ffi::c_void;
use std::path::PathBuf;

pub use self::utils::is_wasi_module;

use std::rc::Rc;
use wasmer_runtime_core::state::x64::read_stack;
use wasmer_runtime_core::vm::Ctx;
use wasmer_runtime_core::{
    func,
    import::ImportObject,
    imports,
    trampoline::{CallContext, TrampolineBufferBuilder},
};

/// This is returned in the Box<dyn Any> RuntimeError::Error variant.
/// Use `downcast` or `downcast_ref` to retrieve the `ExitCode`.
pub struct ExitCode {
    pub code: syscalls::types::__wasi_exitcode_t,
}

pub fn generate_import_object(
    args: Vec<Vec<u8>>,
    envs: Vec<Vec<u8>>,
    preopened_files: Vec<String>,
    mapped_dirs: Vec<(String, PathBuf)>,
) -> ImportObject {
    unsafe extern "C" fn read_stack(ctx: &mut Ctx, _: *const CallContext, mut stack: *const u64) {
        use wasmer_runtime_core::state::x64::{X64Register, GPR};

        let msm = (*ctx.module)
            .runnable_module
            .get_module_state_map()
            .unwrap();
        let code_base = (*ctx.module).runnable_module.get_code().unwrap().as_ptr() as usize;

        let mut known_registers: [Option<u64>; 24] = [None; 24];

        let r15 = *stack;
        let r14 = *stack.offset(1);
        let r13 = *stack.offset(2);
        let r12 = *stack.offset(3);
        let rbx = *stack.offset(4);
        stack = stack.offset(5);

        known_registers[X64Register::GPR(GPR::R15).to_index().0] = Some(r15);
        known_registers[X64Register::GPR(GPR::R14).to_index().0] = Some(r14);
        known_registers[X64Register::GPR(GPR::R13).to_index().0] = Some(r13);
        known_registers[X64Register::GPR(GPR::R12).to_index().0] = Some(r12);
        known_registers[X64Register::GPR(GPR::RBX).to_index().0] = Some(rbx);

        let stack_dump = self::read_stack(&msm, code_base, stack, known_registers, None);
        println!("{:?}", stack_dump);
    }

    let mut builder = TrampolineBufferBuilder::new();
    let idx = builder.add_context_rsp_state_preserving_trampoline(read_stack, ::std::ptr::null());
    let trampolines = builder.build();

    let read_stack_indirect: fn(&mut Ctx) =
        unsafe { ::std::mem::transmute(trampolines.get_trampoline(idx)) };

    let trampolines = Rc::new(trampolines);

    let state_gen = move || {
        fn state_destructor(data: *mut c_void) {
            unsafe {
                drop(Box::from_raw(data as *mut WasiState));
            }
        }

        let state = Box::new(WasiState {
            fs: WasiFs::new(&preopened_files, &mapped_dirs).unwrap(),
            args: &args[..],
            envs: &envs[..],
            trampolines: trampolines.clone(),
        });

        (
            Box::leak(state) as *mut WasiState as *mut c_void,
            state_destructor as fn(*mut c_void),
        )
    };
    imports! {
        // This generates the wasi state.
        state_gen,
        "wasi_unstable" => {
            "stack_read" => func!(read_stack_indirect),
            "args_get" => func!(args_get),
            "args_sizes_get" => func!(args_sizes_get),
            "clock_res_get" => func!(clock_res_get),
            "clock_time_get" => func!(clock_time_get),
            "environ_get" => func!(environ_get),
            "environ_sizes_get" => func!(environ_sizes_get),
            "fd_advise" => func!(fd_advise),
            "fd_allocate" => func!(fd_allocate),
            "fd_close" => func!(fd_close),
            "fd_datasync" => func!(fd_datasync),
            "fd_fdstat_get" => func!(fd_fdstat_get),
            "fd_fdstat_set_flags" => func!(fd_fdstat_set_flags),
            "fd_fdstat_set_rights" => func!(fd_fdstat_set_rights),
            "fd_filestat_get" => func!(fd_filestat_get),
            "fd_filestat_set_size" => func!(fd_filestat_set_size),
            "fd_filestat_set_times" => func!(fd_filestat_set_times),
            "fd_pread" => func!(fd_pread),
            "fd_prestat_get" => func!(fd_prestat_get),
            "fd_prestat_dir_name" => func!(fd_prestat_dir_name),
            "fd_pwrite" => func!(fd_pwrite),
            "fd_read" => func!(fd_read),
            "fd_readdir" => func!(fd_readdir),
            "fd_renumber" => func!(fd_renumber),
            "fd_seek" => func!(fd_seek),
            "fd_sync" => func!(fd_sync),
            "fd_tell" => func!(fd_tell),
            "fd_write" => func!(fd_write),
            "path_create_directory" => func!(path_create_directory),
            "path_filestat_get" => func!(path_filestat_get),
            "path_filestat_set_times" => func!(path_filestat_set_times),
            "path_link" => func!(path_link),
            "path_open" => func!(path_open),
            "path_readlink" => func!(path_readlink),
            "path_remove_directory" => func!(path_remove_directory),
            "path_rename" => func!(path_rename),
            "path_symlink" => func!(path_symlink),
            "path_unlink_file" => func!(path_unlink_file),
            "poll_oneoff" => func!(poll_oneoff),
            "proc_exit" => func!(proc_exit),
            "proc_raise" => func!(proc_raise),
            "random_get" => func!(random_get),
            "sched_yield" => func!(sched_yield),
            "sock_recv" => func!(sock_recv),
            "sock_send" => func!(sock_send),
            "sock_shutdown" => func!(sock_shutdown),
        },
    }
}
