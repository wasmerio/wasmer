use super::*;
use crate::syscalls::*;

/// ### `ws_connect()`
/// Connects to a websocket at a particular network URL
///
/// ## Parameters
///
/// * `url` - URL of the web socket destination to connect to
///
/// ## Return
///
/// Returns a socket handle which is used to send and receive data
pub fn ws_connect<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    url: WasmPtr<u8, M>,
    url_len: M::Offset,
    ret_sock: WasmPtr<WasiFd, M>,
) -> Result<Errno, WasiError> {
    debug!(
        "wasi[{}:{}]::ws_connect",
        ctx.data().pid(),
        ctx.data().tid()
    );
    let mut env = ctx.data();
    let memory = env.memory_view(&ctx);
    let url = unsafe { get_input_str_ok!(&memory, url, url_len) };

    let net = env.net();
    let tasks = env.tasks.clone();
    let socket = wasi_try_ok!(__asyncify(&mut ctx, None, async move {
        net.ws_connect(url.as_str())
            .await
            .map_err(net_error_into_wasi_err)
    })?);
    env = ctx.data();

    let (memory, state, mut inodes) = env.get_memory_and_wasi_state_and_inodes_mut(&ctx, 0);

    let kind = Kind::Socket {
        socket: InodeSocket::new(InodeSocketKind::WebSocket(socket)),
    };

    let inode =
        state
            .fs
            .create_inode_with_default_stat(inodes.deref_mut(), kind, false, "socket".into());
    let rights = Rights::all_socket();
    let fd = wasi_try_ok!(state
        .fs
        .create_fd(rights, rights, Fdflags::empty(), 0, inode));

    wasi_try_mem_ok!(ret_sock.write(&memory, fd));

    Ok(Errno::Success)
}
