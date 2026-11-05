//! WASI masks live on descriptor slots, while the kernel carries only opaque attachments.
//! Authorization admits the exact captured objects; reductions affect later admissions.

use crate::{
    abi::{Errno, fs::Rights},
    errno::V1Result,
    shim::{self, Frame},
};
use wasixv2_abi::Fd;
use wasixv2_syscalls::{Control, DescriptorAttachment, DescriptorUpdate, SysErr, Syscalls};

#[derive(Clone, Copy)]
pub(crate) struct Record {
    pub(crate) base: Rights,
    pub(crate) inheriting: Rights,
    pub(crate) preopen: bool,
    pub(crate) socket: bool,
}

impl Record {
    pub(crate) fn new(base: Rights, inheriting: Rights) -> Self {
        Self {
            base,
            inheriting,
            preopen: false,
            socket: false,
        }
    }

    pub(crate) fn attachment(self) -> Option<DescriptorAttachment> {
        Some(DescriptorAttachment::new(self))
    }
}

pub(crate) async fn record<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    fd: Fd,
) -> V1Result<Record> {
    let set = frame.bind_descriptors(&[fd])?;
    let entry = set
        .get(fd)
        .filter(|entry| entry.present)
        .ok_or(Errno::BADF)?;
    match &entry.attachment {
        Some(attachment) => attachment
            .downcast_ref::<Record>()
            .copied()
            .ok_or_else(|| Errno::NOTSUP.into()),
        None => {
            let stat = crate::calls::fs::infer_fdstat(frame, fd).await?;
            let mut record = Record::new(stat.fs_rights_base, stat.fs_rights_inheriting);
            record.socket = matches!(
                stat.fs_filetype,
                crate::abi::fs::Filetype::SOCKET_STREAM
                    | crate::abi::fs::Filetype::SOCKET_DGRAM
                    | crate::abi::fs::Filetype::SOCKET_RAW
                    | crate::abi::fs::Filetype::SOCKET_SEQPACKET
            );
            Ok(record)
        }
    }
}

/// Capture all operands together before any probe, check or provider/VFS side effect.
pub(crate) async fn require<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    operands: &[(Fd, Rights)],
) -> V1Result<()> {
    frame.bind_descriptors(&operands.iter().map(|(fd, _)| *fd).collect::<Vec<_>>())?;
    for &(fd, rights) in operands {
        if !record(frame, fd).await?.base.contains(rights) {
            return shim::fail(Errno::ACCES);
        }
    }
    Ok(())
}

pub(crate) async fn reduce<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    fd: Fd,
    base: Rights,
    inheriting: Rights,
) -> V1Result<()> {
    loop {
        let set = frame
            .syscalls
            .snapshot_descriptors(frame.task, &[fd])
            .map_err(SysErr::from)?;
        frame.replace_descriptors(set.clone());
        let old = record(frame, fd).await?;
        if !old.base.contains(base) || !old.inheriting.contains(inheriting) {
            return shim::fail(Errno::NOTCAPABLE);
        }
        let new = Record {
            base,
            inheriting,
            ..old
        };
        match frame
            .syscalls
            .replace_descriptor_attachment(frame.task, &set, fd, new.attachment())
            .map_err(SysErr::from)?
        {
            DescriptorUpdate::Replaced => return Ok(()),
            DescriptorUpdate::Conflict => continue,
            DescriptorUpdate::Stale => return shim::fail(Errno::BADF),
        }
    }
}

pub(crate) async fn duplicate<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    fd: Fd,
) -> V1Result<()> {
    let old = record(frame, fd).await?;
    frame.install_descriptors(vec![
        Record {
            preopen: false,
            ..old
        }
        .attachment(),
    ])?;
    Ok(())
}

pub(crate) fn socket() -> Record {
    let rights = Rights::SOCK_ALL
        | Rights::POLL_FD_READWRITE
        | Rights::FD_FDSTAT_SET_FLAGS
        | Rights::FD_FILESTAT_GET
        | Rights::FD_READ
        | Rights::FD_WRITE;
    Record {
        socket: true,
        ..Record::new(rights, rights)
    }
}

/// v1 routes pipe-backed socket calls through ordinary stream I/O. The existing adapter
/// also admits other non-socket streams when no socket-specific flags were supplied.
pub(crate) async fn socket_or_stream<S: Syscalls + Control + 'static>(
    frame: &Frame<'_, S>,
    fd: Fd,
    socket: Rights,
    stream: Rights,
    fallback: bool,
) -> V1Result<()> {
    let record = record(frame, fd).await?;
    let required = if record.socket {
        socket
    } else if fallback {
        stream
    } else {
        Rights::empty()
    };
    if !record.base.contains(required) {
        return shim::fail(Errno::ACCES);
    }
    Ok(())
}
