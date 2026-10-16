//! What a v1 process reads at startup: arguments, environment and preopened directories.
//!
//! v2 hands a process its arguments and environment as one Linux-style startup block
//! (`startup_get`); v1 has `args_*`/`environ_*`. The block is read once per image, into
//! scratch, and served from [`Startup`]. v1 also expects preopened directories, announced
//! through `fd_prestat_get` from fd 3 up. The compat layer opens them as v1's runner did, so
//! they normally land on the same descriptors: the virtual root `/` on 3, the working directory
//! as `.` on 4 and the root again as `/` on 5. Programs and tests that hard-code these numbers
//! keep working.

use std::sync::Arc;

use wasixv2_abi::{AtFd, Fd, OpenFlags};
use wasixv2_syscalls::{Control, SysResult, Syscalls};
use wasmer::RuntimeError;

use crate::{
    abi::{
        Errno,
        fs::{Preopentype, Prestat},
    },
    process::{Preopen, ProcState, Startup},
    shim::{self, Env, Frame},
};

/// The directories every v1 process starts with, as (path, announced name).
const PREOPENS: [(&[u8], &[u8]); 3] = [(b"/", b"/"), (b".", b"."), (b"/", b"/")];

/// The startup state of `proc`, loading it on first use.
pub(crate) async fn load<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    proc: &ProcState,
) -> Arc<Startup> {
    if let Some(startup) = proc.startup.get() {
        return startup.clone();
    }
    let (args, env) = read_block(frame).await.unwrap_or_else(|err| {
        tracing::warn!(
            ?err,
            "cannot read the startup block; no arguments or environment"
        );
        Default::default()
    });
    let startup = Arc::new(Startup {
        args,
        env,
        preopens: open_preopens(frame).await,
    });
    if proc.startup.set(startup.clone()).is_ok() {
        return startup;
    }
    // Another thread of the process won the race; its preopens are the ones the guest sees.
    for preopen in &startup.preopens {
        let _ = frame.syscalls.close(&frame.caller(), preopen.fd).await;
    }
    proc.startup.get().expect("set by the winner").clone()
}

type Strings = Vec<Vec<u8>>;

/// The arguments and environment from the kernel's startup block.
async fn read_block<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
) -> SysResult<(Strings, Strings)> {
    let caller = frame.caller();
    let size = frame.syscalls.startup_size(&caller).await?;
    let buf = frame.mem.alloc_zeroed(size)?;
    frame.syscalls.startup_get(&caller, buf).await?;
    let block = frame.mem.take_bytes(buf, size as usize);
    Ok(parse_block(&block, buf.addr).unwrap_or_default())
}

/// Parses `argc`, `argv[]`, NULL, `envp[]`, NULL from a block loaded at `base`.
fn parse_block(block: &[u8], base: u32) -> Option<(Strings, Strings)> {
    let word = |index: usize| -> Option<u32> {
        let bytes = block.get(index * 4..index * 4 + 4)?;
        Some(u32::from_le_bytes(bytes.try_into().ok()?))
    };
    let string = |ptr: u32| -> Option<Vec<u8>> {
        let start = ptr.checked_sub(base)? as usize;
        let rest = block.get(start..)?;
        let len = rest.iter().position(|&b| b == 0)?;
        Some(rest[..len].to_vec())
    };
    let argc = word(0)? as usize;
    let args = (1..=argc)
        .map(|i| word(i).and_then(string))
        .collect::<Option<Vec<_>>>()?;
    let mut env = Vec::new();
    let mut index = argc + 2;
    loop {
        match word(index)? {
            0 => break,
            ptr => env.push(string(ptr)?),
        }
        index += 1;
    }
    Some((args, env))
}

async fn open_preopens<S: Syscalls + Control + 'static>(frame: &Frame<'_, S>) -> Vec<Preopen> {
    let mut preopens = Vec::new();
    for (path, name) in PREOPENS {
        match open_dir(frame, path).await {
            Ok(fd) => preopens.push(Preopen {
                fd,
                name: name.to_vec(),
            }),
            Err(err) => {
                tracing::warn!(?err, path = %String::from_utf8_lossy(path), "cannot preopen")
            }
        }
    }
    preopens
}

async fn open_dir<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    path: &[u8],
) -> SysResult<Fd> {
    let flags = OpenFlags::O_RDONLY | OpenFlags::O_DIRECTORY | OpenFlags::O_CLOEXEC;
    let path = frame.mem.alloc_cstr(path)?;
    frame
        .syscalls
        .openat(&frame.caller(), Fd(AtFd::AT_FDCWD.0), path, flags, 0)
        .await
}

/// Total bytes of `strings` with a NUL after each.
fn buf_size(strings: &[Vec<u8>]) -> u32 {
    strings.iter().map(|s| s.len() as u32 + 1).sum()
}

/// Writes `strings` NUL-terminated into the guest buffer at `buf`, and their addresses into
/// the pointer array at `pointers`.
async fn write_strings<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    strings: &[Vec<u8>],
    pointers: u32,
    buf: u32,
) -> crate::errno::V1Result<()> {
    let mut bytes = Vec::with_capacity(buf_size(strings) as usize);
    let mut addrs = Vec::with_capacity(strings.len() * 4);
    for string in strings {
        let addr = buf.wrapping_add(bytes.len() as u32);
        addrs.extend_from_slice(&addr.to_le_bytes());
        bytes.extend_from_slice(string);
        bytes.push(0);
    }
    frame.write_bytes(buf, &bytes).await?;
    frame.write_bytes(pointers, &addrs).await
}

pub(crate) async fn args_sizes_get<S: Syscalls + Control + 'static>(
    env: Env<S>,
    count: i32,
    buf_size_out: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let startup = frame.startup().await?;
        frame
            .write(count as u32, &(startup.args.len() as u32))
            .await?;
        frame
            .write(buf_size_out as u32, &buf_size(&startup.args))
            .await
    })
    .await
}

pub(crate) async fn args_get<S: Syscalls + Control + 'static>(
    env: Env<S>,
    argv: i32,
    buf: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let startup = frame.startup().await?;
        write_strings(frame, &startup.args, argv as u32, buf as u32).await
    })
    .await
}

pub(crate) async fn environ_sizes_get<S: Syscalls + Control + 'static>(
    env: Env<S>,
    count: i32,
    buf_size_out: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let startup = frame.startup().await?;
        frame
            .write(count as u32, &(startup.env.len() as u32))
            .await?;
        frame
            .write(buf_size_out as u32, &buf_size(&startup.env))
            .await
    })
    .await
}

pub(crate) async fn environ_get<S: Syscalls + Control + 'static>(
    env: Env<S>,
    environ: i32,
    buf: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let startup = frame.startup().await?;
        write_strings(frame, &startup.env, environ as u32, buf as u32).await
    })
    .await
}

/// `EBADF` for anything but a preopen ends wasix-libc's scan for them.
pub(crate) async fn fd_prestat_get<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    out: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let startup = frame.startup().await?;
        let Some(preopen) = startup.preopen(Fd(fd)) else {
            return shim::fail(Errno::BADF);
        };
        if !crate::calls::fs::is_preopen(frame, fd).await? {
            return shim::fail(Errno::BADF);
        }
        let prestat = Prestat {
            pr_type: Preopentype::DIR,
            pr_name_len: preopen.name.len() as u32,
        };
        frame.write(out as u32, &prestat).await
    })
    .await
}

/// Writes the preopen's name without a NUL; `OVERFLOW` if it does not fit.
pub(crate) async fn fd_prestat_dir_name<S: Syscalls + Control + 'static>(
    env: Env<S>,
    fd: i32,
    path: i32,
    path_len: i32,
) -> Result<i32, RuntimeError> {
    shim::errno(&env, async |frame| {
        let startup = frame.startup().await?;
        let Some(preopen) = startup.preopen(Fd(fd)) else {
            return shim::fail(Errno::BADF);
        };
        if !crate::calls::fs::is_preopen(frame, fd).await? {
            return shim::fail(Errno::BADF);
        }
        if preopen.name.len() > path_len as u32 as usize {
            return shim::fail(Errno::OVERFLOW);
        }
        frame.write_bytes(path as u32, &preopen.name).await
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_startup_block() {
        let base = 0x1000;
        // argc, argv[0], NULL, envp[0], NULL, auxv (AT_NULL), strings
        let mut block = Vec::new();
        for word in [1u32, base + 32, 0, base + 35, 0, 0, 0, 0] {
            block.extend_from_slice(&word.to_le_bytes());
        }
        block.extend_from_slice(b"ab\0K=V\0");
        let (args, env) = parse_block(&block, base).unwrap();
        assert_eq!(args, vec![b"ab".to_vec()]);
        assert_eq!(env, vec![b"K=V".to_vec()]);
    }
}
