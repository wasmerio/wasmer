//! The SCTP socket options the socket layer serves itself (`net/sctp/socket.c`): those that
//! carry packed address arrays (`SCTP_SOCKOPT_BINDX_*`, `SCTP_SOCKOPT_CONNECTX*`), block like
//! `connect`, or make descriptors (`SCTP_SOCKOPT_PEELOFF*`). Every other `SOL_SCTP` option is
//! the protocol's; its `getsockopt` value is read first, since most name an association or
//! an address in the value they fill in.

use std::sync::Arc;

use wasixv2_abi::{
    Buf, GuestLayout, MsgFlags, SctpGetaddrsOld, SctpOption, SctpPeeloffArg, SctpPeeloffFlagsArg,
    SocketFlags, SocketLevel, SocklenPtr,
};
use wasixv2_syscalls::{Caller, Errno, SysResult, write_value};
use wasixv2_vfs::Cap;

use super::{OPTMEM_MAX, Sock};
use crate::{
    Kernel,
    file::OpenFile,
    socket::{self, SND, SocketFile},
};

/// `KMALLOC_MAX_SIZE` on 64-bit with 4 KiB pages: the most `memdup_user` takes.
const KMALLOC_MAX_SIZE: usize = 4 << 20;

impl Kernel {
    /// The `SOL_SCTP` `setsockopt`s the kernel serves (`sctp_setsockopt`'s first cases), or
    /// `None` for the protocol's. `CONNECTX` returns the new association's id.
    pub(super) async fn sctp_setsockopt(
        &self,
        caller: &Caller<'_>,
        file: &OpenFile,
        sock: &SocketFile,
        name: i32,
        value: &[u8],
    ) -> Option<SysResult<i32>> {
        let ops = sock.socket.sctp()?;
        Some(match SctpOption(name) {
            SctpOption::SCTP_SOCKOPT_BINDX_ADD | SctpOption::SCTP_SOCKOPT_BINDX_REM => {
                let add = SctpOption(name) == SctpOption::SCTP_SOCKOPT_BINDX_ADD;
                let privileged = self.cred(caller).capable(Cap::NetBindService);
                socket::decode_sctp_packed(value)
                    .and_then(|addrs| ops.bindx(&addrs, add, privileged))
                    .map(|()| 0)
                    .map_err(Into::into)
            }
            SctpOption::SCTP_SOCKOPT_CONNECTX_OLD | SctpOption::SCTP_SOCKOPT_CONNECTX => {
                match self.sctp_connectx(caller, file, sock, value) {
                    Ok((_, true)) => Err(Errno::EINPROGRESS.into()),
                    // `sctp_setsockopt_connectx` returns the id; the old option nothing.
                    Ok((id, false)) if SctpOption(name) == SctpOption::SCTP_SOCKOPT_CONNECTX => {
                        Ok(id)
                    }
                    Ok(_) => Ok(0),
                    Err(err) => Err(err.into()),
                }
            }
            _ => return None,
        })
    }

    /// `__sctp_setsockopt_connectx`: the association's id, and whether the caller is
    /// non-blocking and so hears `EINPROGRESS` (`sctp_wait_for_connect`), though the
    /// association is up when `connectx` returns.
    fn sctp_connectx(
        &self,
        caller: &Caller<'_>,
        file: &OpenFile,
        sock: &SocketFile,
        raw: &[u8],
    ) -> Result<(i32, bool), Errno> {
        let ops = sock.socket.sctp().expect("an SCTP socket");
        // `__sctp_setsockopt_connectx`: at least the first address's family.
        if raw.len() < 2 {
            return Err(Errno::EINVAL);
        }
        let addrs = socket::decode_sctp_packed(raw)?;
        let id = ops.connectx(&addrs)?;
        let interrupt = self.interrupt(caller);
        let wait = sock.blocking(Self::io(caller, &*interrupt, file), SND, MsgFlags(0));
        Ok((id, wait.nonblocking()))
    }

    /// `sctp_getsockopt`: the descriptor-making and connecting options here, the rest from the
    /// protocol over the caller's value. Returns the new descriptor for a peeloff.
    pub(super) async fn sctp_getsockopt(
        &self,
        caller: &Caller<'_>,
        (file, sock): &Sock,
        name: i32,
        len: usize,
        (optval, optlen): (Buf, SocklenPtr),
    ) -> SysResult<i32> {
        let memory = caller.memory();
        let out = (optval, optlen);
        match SctpOption(name) {
            SctpOption::SCTP_SOCKOPT_PEELOFF => {
                if len < SctpPeeloffArg::SIZE {
                    return Err(Errno::EINVAL.into());
                }
                let arg: SctpPeeloffArg = read_record(caller, optval).await?;
                let flags = SocketFlags(0);
                self.sctp_peeloff(caller, sock, arg, flags, out, |arg| arg)
                    .await
            }
            SctpOption::SCTP_SOCKOPT_PEELOFF_FLAGS => {
                if len < SctpPeeloffFlagsArg::SIZE {
                    return Err(Errno::EINVAL.into());
                }
                let arg: SctpPeeloffFlagsArg = read_record(caller, optval).await?;
                let flags = SocketFlags(arg.flags);
                let back = move |p_arg| SctpPeeloffFlagsArg { p_arg, ..arg };
                self.sctp_peeloff(caller, sock, arg.p_arg, flags, out, back)
                    .await
            }
            SctpOption::SCTP_SOCKOPT_CONNECTX3 => {
                if len < SctpGetaddrsOld::SIZE {
                    return Err(Errno::EINVAL.into());
                }
                let param: SctpGetaddrsOld = read_record(caller, optval).await?;
                // `memdup_user` of `addr_num` bytes.
                let size = usize::try_from(param.addr_num).map_err(|_| Errno::ENOMEM)?;
                if size > KMALLOC_MAX_SIZE {
                    return Err(Errno::ENOMEM.into());
                }
                let mut raw = vec![0; size];
                memory.read(param.addrs.addr(), &mut raw).await?;
                // The id goes back over the value even to a non-blocking caller.
                let (id, nonblocking) = self.sctp_connectx(caller, file, sock, &raw)?;
                memory.write(optval.addr(), &id.to_le_bytes()).await?;
                write_value(memory, optlen, &4u32).await?;
                match nonblocking {
                    true => Err(Errno::EINPROGRESS.into()),
                    false => Ok(0),
                }
            }
            _ => {
                let mut out = vec![0; len.min(OPTMEM_MAX)];
                memory.read(optval.addr(), &mut out).await?;
                let n = sock
                    .socket
                    .get_option(SocketLevel::SOL_SCTP.0, name, &mut out)?;
                memory.write(optval.addr(), &out[..n]).await?;
                write_value(memory, optlen, &(n as u32)).await?;
                Ok(0)
            }
        }
    }

    /// `sctp_getsockopt_peeloff_common`: the descriptor is reserved first and installed only
    /// once the value and its length are written back, so a fault leaves no descriptor.
    async fn sctp_peeloff<T: GuestLayout>(
        &self,
        caller: &Caller<'_>,
        sock: &SocketFile,
        mut arg: SctpPeeloffArg,
        flags: SocketFlags,
        (optval, optlen): (Buf, SocklenPtr),
        back: impl FnOnce(SctpPeeloffArg) -> T,
    ) -> SysResult<i32> {
        // `sctp_do_peeloff`: never from one network namespace into another.
        if !Arc::ptr_eq(&self.ns(caller).net, &sock.net) {
            return Err(Errno::EINVAL.into());
        }
        let ops = sock.socket.sctp().expect("an SCTP socket");
        let peeled = ops.peeloff(arg.associd)?;
        let cloexec = flags.contains(SocketFlags::SOCK_CLOEXEC);
        let [slot] = self.reserve_fds::<1>(caller, cloexec)?;
        let file = self.socket_file(caller, peeled, sock.net.clone(), flags);
        arg.sd = slot.fd().0;
        let value = back(arg);
        let mut raw = vec![0; T::SIZE];
        value.write_to(&mut raw);
        write_value(caller.memory(), optlen, &(T::SIZE as u32)).await?;
        caller.memory().write(optval.addr(), &raw).await?;
        Ok(slot.install(Arc::new(file)).0)
    }
}

/// A record at the start of a `getsockopt` value.
async fn read_record<T: GuestLayout>(caller: &Caller<'_>, optval: Buf) -> SysResult<T> {
    let mut raw = vec![0; T::SIZE];
    caller.memory().read(optval.addr(), &mut raw).await?;
    Ok(T::read_from(&raw).expect("of its size"))
}
