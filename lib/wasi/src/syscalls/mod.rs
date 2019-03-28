use crate::state::WasiState;
use wasmer_runtime_core::{memory::Memory, vm::Ctx};

#[allow(clippy::mut_from_ref)]
fn get_wasi_state(ctx: &Ctx) -> &mut WasiState {
    unsafe { &mut *(ctx.data as *mut WasiState) }
}

fn write_buffer_array(
    memory: &Memory,
    from: &[Vec<u8>],
    ptr_buffer_offset: u32,
    buffer_offset: u32,
) {
    let mut current_buffer_offset = buffer_offset;
    for (i, sub_buffer) in from.iter().enumerate() {
        memory.view::<u32>()[(ptr_buffer_offset as usize)..][i].set(current_buffer_offset);
        for (cell, &byte) in memory.view()[(current_buffer_offset as usize)..]
            .iter()
            .zip(sub_buffer.iter())
        {
            cell.set(byte);
        }
        current_buffer_offset += sub_buffer.len() as u32;
    }
}

/// ### `__wasi_args_get()`
/// Read command-line argument data.
/// The sizes of the buffers should match that returned by [`__wasi_args_sizes_get()`](#args_sizes_get).
/// Inputs:
/// - `char **argv`
///     A pointer to a buffer to write the argument pointers.
/// - `char *argv_buf`
///     A pointer to a buffer to write the argument string data.
///
pub fn __wasi_args_get(ctx: &mut Ctx, ptr_buffer_offset: u32, buffer_offset: u32) {
    let state = get_wasi_state(ctx);
    let memory = ctx.memory(0);

    write_buffer_array(memory, &*state.args, ptr_buffer_offset, buffer_offset);
}

/// ### `__wasi_args_sizes_get()`
/// Return command-line argument data sizes.
/// Outputs:
/// - `size_t *argc`
///     The number of arguments.
/// - `size_t *argv_buf_size`
///     The size of the argument string data.
pub fn __wasi_args_sizes_get(ctx: &mut Ctx, argc_out: u32, argv_buf_size_out: u32) {
    let state = get_wasi_state(ctx);
    let memory = ctx.memory(0);

    let arg_count = state.args.len();
    let total_arg_size: usize = state.args.iter().map(|v| v.len()).sum();

    memory.view::<u32>()[(argc_out / 4) as usize].set(arg_count as u32);
    memory.view::<u32>()[(argv_buf_size_out / 4) as usize].set(total_arg_size as u32);
}

pub fn __wasi_clock_res_get(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_clock_time_get(ctx: &mut Ctx) {
    unimplemented!()
}

/// ### `__wasi_environ_get()`
/// Read environment variable data.
/// The sizes of the buffers should match that returned by [`__wasi_environ_sizes_get()`](#environ_sizes_get).
/// Inputs:
/// - `char **environ`
///     A pointer to a buffer to write the environment variable pointers.
/// - `char *environ_buf`
///     A pointer to a buffer to write the environment variable string data.
pub fn __wasi_environ_get(ctx: &mut Ctx, environ: u32, environ_buf: u32) {
    let state = get_wasi_state(ctx);
    let memory = ctx.memory(0);

    write_buffer_array(memory, &*state.args, environ, environ_buf);
}

/// ### `__wasi_environ_sizes_get()`
/// Return command-line argument data sizes.
/// Outputs:
/// - `size_t *environ_count`
///     The number of environment variables.
/// - `size_t *environ_buf_size`
///     The size of the environment variable string data.
pub fn __wasi_environ_sizes_get(ctx: &mut Ctx, environ_count_out: u32, environ_buf_size_out: u32) {
    let state = get_wasi_state(ctx);
    let memory = ctx.memory(0);

    let env_count = state.envs.len();
    let total_env_size: usize = state.envs.iter().map(|v| v.len()).sum();

    memory.view::<u32>()[(environ_count_out / 4) as usize].set(env_count as u32);
    memory.view::<u32>()[(environ_buf_size_out / 4) as usize].set(total_env_size as u32);
}

pub fn __wasi_fd_advise(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_fd_allocate(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_fd_close(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_fd_datasync(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_fd_fdstat_get(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_fd_fdstat_set_flags(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_fd_fdstat_set_rights(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_fd_filestat_get(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_fd_filestat_set_size(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_fd_filestat_set_times(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_fd_pread(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_fd_prestat_get(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_fd_prestat_dir_name(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_fd_pwrite(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_fd_read(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_fd_readdir(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_fd_renumber(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_fd_seek(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_fd_sync(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_fd_tell(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_fd_write(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_path_create_directory(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_path_filestat_get(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_path_filestat_set_times(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_path_link(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_path_open(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_path_readlink(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_path_remove_directory(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_path_rename(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_path_symlink(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_path_unlink_file(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_poll_oneoff(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_proc_exit(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_proc_raise(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_random_get(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_sched_yield(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_sock_recv(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_sock_send(ctx: &mut Ctx) {
    unimplemented!()
}
pub fn __wasi_sock_shutdown(ctx: &mut Ctx) {
    unimplemented!()
}
