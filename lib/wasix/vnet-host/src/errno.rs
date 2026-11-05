use std::io;

use wasixv2_abi::Errno;

/// Maps a host error to the Linux errno the guest sees. On Linux the host's numbers are the
/// guest's (both use the generic table); elsewhere the error kind decides.
pub fn errno_from_io(e: &io::Error) -> Errno {
    #[cfg(target_os = "linux")]
    if let Some(raw) = e.raw_os_error() {
        return Errno(raw);
    }
    use io::ErrorKind as K;
    match e.kind() {
        K::ConnectionRefused => Errno::ECONNREFUSED,
        K::ConnectionReset => Errno::ECONNRESET,
        K::ConnectionAborted => Errno::ECONNABORTED,
        K::NotConnected => Errno::ENOTCONN,
        K::AddrInUse => Errno::EADDRINUSE,
        K::AddrNotAvailable => Errno::EADDRNOTAVAIL,
        K::BrokenPipe => Errno::EPIPE,
        K::WouldBlock => Errno::EAGAIN,
        K::TimedOut => Errno::ETIMEDOUT,
        K::Interrupted => Errno::EINTR,
        K::PermissionDenied => Errno::EACCES,
        K::InvalidInput => Errno::EINVAL,
        K::HostUnreachable => Errno::EHOSTUNREACH,
        K::NetworkUnreachable => Errno::ENETUNREACH,
        K::NetworkDown => Errno::ENETDOWN,
        K::OutOfMemory => Errno::ENOMEM,
        K::Unsupported => Errno::EOPNOTSUPP,
        _ => Errno::EIO,
    }
}
