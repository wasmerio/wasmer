//! v1 error numbers: WASI's `errno` (a `u16`, alphabetical) plus the WASIX additions after
//! `NOTCAPABLE`, and the translation from Linux's.

use wasixv2_abi::Errno as L;

/// A v1 `errno`. Imports return it as an `i32`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Errno(pub u16);

macro_rules! errnos {
    ($($name:ident = $value:literal => [$($linux:ident),*]),* $(,)?) => {
        impl Errno {
            $(pub const $name: Self = Self($value);)*

            /// Every v1 errno with its name, in numeric order.
            pub const ALL: &'static [(&'static str, Self)] = &[$((stringify!($name), Self::$name)),*];

            /// The v1 errno for a Linux one. Linux errnos WASI has no name for are `IO`.
            pub fn from_linux(errno: L) -> Self {
                $($(if errno == L::$linux { return Self::$name; })*)*
                Self::IO
            }
        }
    };
}

errnos! {
    SUCCESS = 0 => [],
    TOOBIG = 1 => [E2BIG],
    ACCES = 2 => [EACCES],
    ADDRINUSE = 3 => [EADDRINUSE],
    ADDRNOTAVAIL = 4 => [EADDRNOTAVAIL],
    AFNOSUPPORT = 5 => [EAFNOSUPPORT, EPFNOSUPPORT],
    AGAIN = 6 => [EAGAIN],
    ALREADY = 7 => [EALREADY],
    BADF = 8 => [EBADF, EBADFD],
    BADMSG = 9 => [EBADMSG],
    BUSY = 10 => [EBUSY],
    CANCELED = 11 => [ECANCELED],
    CHILD = 12 => [ECHILD],
    CONNABORTED = 13 => [ECONNABORTED],
    CONNREFUSED = 14 => [ECONNREFUSED],
    CONNRESET = 15 => [ECONNRESET],
    DEADLK = 16 => [EDEADLK],
    DESTADDRREQ = 17 => [EDESTADDRREQ],
    DOM = 18 => [EDOM],
    DQUOT = 19 => [EDQUOT],
    EXIST = 20 => [EEXIST],
    // The v1 host reported a bad guest pointer as `MEMVIOLATION`, never as `FAULT`.
    FAULT = 21 => [],
    FBIG = 22 => [EFBIG],
    HOSTUNREACH = 23 => [EHOSTUNREACH, EHOSTDOWN],
    IDRM = 24 => [EIDRM],
    ILSEQ = 25 => [EILSEQ],
    INPROGRESS = 26 => [EINPROGRESS],
    INTR = 27 => [EINTR],
    INVAL = 28 => [EINVAL],
    IO = 29 => [EIO],
    ISCONN = 30 => [EISCONN],
    ISDIR = 31 => [EISDIR],
    LOOP = 32 => [ELOOP],
    MFILE = 33 => [EMFILE],
    MLINK = 34 => [EMLINK],
    MSGSIZE = 35 => [EMSGSIZE],
    MULTIHOP = 36 => [EMULTIHOP],
    NAMETOOLONG = 37 => [ENAMETOOLONG],
    NETDOWN = 38 => [ENETDOWN],
    NETRESET = 39 => [ENETRESET],
    NETUNREACH = 40 => [ENETUNREACH],
    NFILE = 41 => [ENFILE],
    NOBUFS = 42 => [ENOBUFS],
    NODEV = 43 => [ENODEV],
    NOENT = 44 => [ENOENT],
    NOEXEC = 45 => [ENOEXEC],
    NOLCK = 46 => [ENOLCK],
    NOLINK = 47 => [ENOLINK],
    NOMEM = 48 => [ENOMEM],
    NOMSG = 49 => [ENOMSG],
    NOPROTOOPT = 50 => [ENOPROTOOPT],
    NOSPC = 51 => [ENOSPC],
    NOSYS = 52 => [ENOSYS],
    NOTCONN = 53 => [ENOTCONN],
    NOTDIR = 54 => [ENOTDIR],
    NOTEMPTY = 55 => [ENOTEMPTY],
    NOTRECOVERABLE = 56 => [ENOTRECOVERABLE],
    NOTSOCK = 57 => [ENOTSOCK],
    NOTSUP = 58 => [EOPNOTSUPP, ESOCKTNOSUPPORT],
    NOTTY = 59 => [ENOTTY],
    NXIO = 60 => [ENXIO],
    OVERFLOW = 61 => [EOVERFLOW],
    OWNERDEAD = 62 => [EOWNERDEAD],
    PERM = 63 => [EPERM],
    PIPE = 64 => [EPIPE],
    PROTO = 65 => [EPROTO],
    PROTONOSUPPORT = 66 => [EPROTONOSUPPORT],
    PROTOTYPE = 67 => [EPROTOTYPE],
    RANGE = 68 => [ERANGE],
    ROFS = 69 => [EROFS],
    SPIPE = 70 => [ESPIPE],
    SRCH = 71 => [ESRCH],
    STALE = 72 => [ESTALE],
    TIMEDOUT = 73 => [ETIMEDOUT],
    TXTBSY = 74 => [ETXTBSY],
    XDEV = 75 => [EXDEV],
    NOTCAPABLE = 76 => [],
    SHUTDOWN = 77 => [ESHUTDOWN],
    MEMVIOLATION = 78 => [EFAULT],
    UNKNOWN = 79 => [],
}

super::newtype_layout!(Errno(u16));

impl From<L> for Errno {
    fn from(errno: L) -> Self {
        Self::from_linux(errno)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbering_is_dense() {
        for (i, (_, errno)) in Errno::ALL.iter().enumerate() {
            assert_eq!(errno.0 as usize, i);
        }
    }

    #[test]
    fn every_linux_errno_maps() {
        // Aliases (`EWOULDBLOCK`, `EDEADLOCK`) share a number with their primary name.
        assert_eq!(Errno::from_linux(L::EWOULDBLOCK), Errno::AGAIN);
        assert_eq!(Errno::from_linux(L::EDEADLOCK), Errno::DEADLK);
        assert_eq!(Errno::from_linux(L::ENOENT), Errno::NOENT);
        assert_eq!(Errno::from_linux(L::EFAULT), Errno::MEMVIOLATION);
        assert_eq!(Errno::from_linux(L::ERESTART), Errno::IO);
        for linux in 1..=134 {
            let v1 = Errno::from_linux(L(linux));
            assert!(v1 != Errno::SUCCESS, "errno {linux} maps to success");
        }
    }
}
