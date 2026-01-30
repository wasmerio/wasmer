use crate::vfs_error_kind_to_wasi_errno;
use vfs_core::VfsErrorKind;
use wasmer_wasix_types::wasi::Errno;

#[test]
fn errno_mapping_is_stable() {
    let cases = [
        (VfsErrorKind::NotFound, Errno::Noent),
        (VfsErrorKind::NotDir, Errno::Notdir),
        (VfsErrorKind::IsDir, Errno::Isdir),
        (VfsErrorKind::AlreadyExists, Errno::Exist),
        (VfsErrorKind::DirNotEmpty, Errno::Notempty),
        (VfsErrorKind::PermissionDenied, Errno::Access),
        (VfsErrorKind::OperationNotPermitted, Errno::Perm),
        (VfsErrorKind::InvalidInput, Errno::Inval),
        (VfsErrorKind::TooManySymlinks, Errno::Loop),
        (VfsErrorKind::NotSupported, Errno::Notsup),
        (VfsErrorKind::CrossDevice, Errno::Xdev),
        (VfsErrorKind::Busy, Errno::Busy),
        (VfsErrorKind::ReadOnlyFs, Errno::Rofs),
        (VfsErrorKind::WouldBlock, Errno::Again),
        (VfsErrorKind::Interrupted, Errno::Intr),
        (VfsErrorKind::TimedOut, Errno::Timedout),
        (VfsErrorKind::Cancelled, Errno::Canceled),
        (VfsErrorKind::BrokenPipe, Errno::Pipe),
        (VfsErrorKind::NoSpace, Errno::Nospc),
        (VfsErrorKind::QuotaExceeded, Errno::Dquot),
        (VfsErrorKind::TooManyOpenFiles, Errno::Mfile),
        (VfsErrorKind::TooManyOpenFilesSystem, Errno::Nfile),
        (VfsErrorKind::TooManyLinks, Errno::Mlink),
        (VfsErrorKind::NameTooLong, Errno::Nametoolong),
        (VfsErrorKind::FileTooLarge, Errno::Fbig),
        (VfsErrorKind::NoMemory, Errno::Nomem),
        (VfsErrorKind::Overflow, Errno::Overflow),
        (VfsErrorKind::Range, Errno::Range),
        (VfsErrorKind::NotSeekable, Errno::Spipe),
        (VfsErrorKind::NotImplemented, Errno::Nosys),
        (VfsErrorKind::Stale, Errno::Stale),
        (VfsErrorKind::Deadlock, Errno::Deadlk),
        (VfsErrorKind::TextFileBusy, Errno::Txtbsy),
        (VfsErrorKind::NoDevice, Errno::Nodev),
        (VfsErrorKind::NoAddress, Errno::Nxio),
        (VfsErrorKind::BadHandle, Errno::Badf),
        (VfsErrorKind::Internal, Errno::Io),
        (VfsErrorKind::Io, Errno::Io),
    ];

    for (kind, expected) in cases {
        assert_eq!(vfs_error_kind_to_wasi_errno(kind), expected);
    }
}
