use super::*;
use crate::syscalls::*;

/// ### `http_request()`
/// Makes a HTTP request to a remote web resource and
/// returns a socket handles that are used to send and receive data
///
/// ## Parameters
///
/// * `url` - URL of the HTTP resource to connect to
/// * `method` - HTTP method to be invoked
/// * `headers` - HTTP headers to attach to the request
///   (headers seperated by lines)
/// * `gzip` - Should the request body be compressed
///
/// ## Return
///
/// The body of the response can be streamed from the returned
/// file handle
pub fn http_request<M: MemorySize>(
    mut ctx: FunctionEnvMut<'_, WasiEnv>,
    url: WasmPtr<u8, M>,
    url_len: M::Offset,
    method: WasmPtr<u8, M>,
    method_len: M::Offset,
    headers: WasmPtr<u8, M>,
    headers_len: M::Offset,
    gzip: Bool,
    ret_handles: WasmPtr<HttpHandles, M>,
) -> Errno {
    debug!(
        "wasi[{}:{}]::http_request",
        ctx.data().pid(),
        ctx.data().tid()
    );
    let mut env = ctx.data();
    let memory = env.memory_view(&ctx);
    let url = unsafe { get_input_str!(&memory, url, url_len) };
    let method = unsafe { get_input_str!(&memory, method, method_len) };
    let headers = unsafe { get_input_str!(&memory, headers, headers_len) };

    let gzip = match gzip {
        Bool::False => false,
        Bool::True => true,
        _ => return Errno::Inval,
    };

    let net = env.net();
    let tasks = env.tasks.clone();
    let socket = wasi_try!(__asyncify(&mut ctx, None, async move {
        net.http_request(url.as_str(), method.as_str(), headers.as_str(), gzip)
            .await
            .map_err(net_error_into_wasi_err)
    }));
    env = ctx.data();

    let socket_req = SocketHttpRequest {
        request: socket.request,
        response: None,
        headers: None,
        status: socket.status.clone(),
    };
    let socket_res = SocketHttpRequest {
        request: None,
        response: socket.response,
        headers: None,
        status: socket.status.clone(),
    };
    let socket_hdr = SocketHttpRequest {
        request: None,
        response: None,
        headers: socket.headers,
        status: socket.status,
    };

    let (memory, state, mut inodes) = env.get_memory_and_wasi_state_and_inodes_mut(&ctx, 0);

    let kind_req = Kind::Socket {
        socket: InodeSocket::new(InodeSocketKind::HttpRequest(
            Mutex::new(socket_req),
            InodeHttpSocketType::Request,
        )),
    };
    let kind_res = Kind::Socket {
        socket: InodeSocket::new(InodeSocketKind::HttpRequest(
            Mutex::new(socket_res),
            InodeHttpSocketType::Response,
        )),
    };
    let kind_hdr = Kind::Socket {
        socket: InodeSocket::new(InodeSocketKind::HttpRequest(
            Mutex::new(socket_hdr),
            InodeHttpSocketType::Headers,
        )),
    };

    let inode_req = state.fs.create_inode_with_default_stat(
        inodes.deref_mut(),
        kind_req,
        false,
        "http_request".to_string().into(),
    );
    let inode_res = state.fs.create_inode_with_default_stat(
        inodes.deref_mut(),
        kind_res,
        false,
        "http_response".to_string().into(),
    );
    let inode_hdr = state.fs.create_inode_with_default_stat(
        inodes.deref_mut(),
        kind_hdr,
        false,
        "http_headers".to_string().into(),
    );
    let rights = Rights::all_socket();

    let handles = HttpHandles {
        req: wasi_try!(state
            .fs
            .create_fd(rights, rights, Fdflags::empty(), 0, inode_req)),
        res: wasi_try!(state
            .fs
            .create_fd(rights, rights, Fdflags::empty(), 0, inode_res)),
        hdr: wasi_try!(state
            .fs
            .create_fd(rights, rights, Fdflags::empty(), 0, inode_hdr)),
    };

    wasi_try_mem!(ret_handles.write(&memory, handles));

    Errno::Success
}
