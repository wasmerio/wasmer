// use std::collections::HashMap;
use crate::EmEnv;
use wasmer::FunctionEnvMut;

pub fn ___seterrno(mut _ctx: FunctionEnvMut<EmEnv>, _value: i32) {
    debug!("emscripten::___seterrno {}", _value);
    // TODO: Incomplete impl
    eprintln!("failed to set errno!");
    // value
}

// pub enum ErrnoCodes {
//     EPERM = 1,
//     ENOENT = 2,
//     ESRCH = 3,
//     EINTR = 4,
//     EIO = 5,
//     ENXIO = 6,
//     E2BIG = 7,
//     ENOEXEC = 8,
//     EBADF = 9,
//     ECHILD = 10,
//     EAGAIN = 11,
//     EWOULDBLOCK = 11,
//     ENOMEM = 12,
//     EACCES = 13,
//     EFAULT = 14,
//     ENOTBLK = 15,
//     EBUSY = 16,
//     EEXIST = 17,
//     EXDEV = 18,
//     ENODEV = 19,
//     ENOTDIR = 20,
//     EISDIR = 21,
//     EINVAL = 22,
//     ENFILE = 23,
//     EMFILE = 24,
//     ENOTTY = 25,
//     ETXTBSY = 26,
//     EFBIG = 27,
//     ENOSPC = 28,
//     ESPIPE = 29,
//     EROFS = 30,
//     EMLINK = 31,
//     EPIPE = 32,
//     EDOM = 33,
//     ERANGE = 34,
//     ENOMSG = 42,
//     EIDRM = 43,
//     ECHRNG = 44,
//     EL2NSYNC = 45,
//     EL3HLT = 46,
//     EL3RST = 47,
//     ELNRNG = 48,
//     EUNATCH = 49,
//     ENOCSI = 50,
//     EL2HLT = 51,
//     EDEADLK = 35,
//     ENOLCK = 37,
//     EBADE = 52,
//     EBADR = 53,
//     EXFULL = 54,
//     ENOANO = 55,
//     EBADRQC = 56,
//     EBADSLT = 57,
//     EDEADLOCK = 35,
//     EBFONT = 59,
//     ENOSTR = 60,
//     ENODATA = 61,
//     ETIME = 62,
//     ENOSR = 63,
//     ENONET = 64,
//     ENOPKG = 65,
//     EREMOTE = 66,
//     ENOLINK = 67,
//     EADV = 68,
//     ESRMNT = 69,
//     ECOMM = 70,
//     EPROTO = 71,
//     EMULTIHOP = 72,
//     EDOTDOT = 73,
//     EBADMSG = 74,
//     ENOTUNIQ = 76,
//     EBADFD = 77,
//     EREMCHG = 78,
//     ELIBACC = 79,
//     ELIBBAD = 80,
//     ELIBSCN = 81,
//     ELIBMAX = 82,
//     ELIBEXEC = 83,
//     ENOSYS = 38,
//     ENOTEMPTY = 39,
//     ENAMETOOLONG = 36,
//     ELOOP = 40,
//     EOPNOTSUPP = 95,
//     EPFNOSUPPORT = 96,
//     ECONNRESET = 104,
//     ENOBUFS = 105,
//     EAFNOSUPPORT = 97,
//     EPROTOTYPE = 91,
//     ENOTSOCK = 88,
//     ENOPROTOOPT = 92,
//     ESHUTDOWN = 108,
//     ECONNREFUSED = 111,
//     EADDRINUSE = 98,
//     ECONNABORTED = 103,
//     ENETUNREACH = 101,
//     ENETDOWN = 100,
//     ETIMEDOUT = 110,
//     EHOSTDOWN = 112,
//     EHOSTUNREACH = 113,
//     EINPROGRESS = 115,
//     EALREADY = 114,
//     EDESTADDRREQ = 89,
//     EMSGSIZE = 90,
//     EPROTONOSUPPORT = 93,
//     ESOCKTNOSUPPORT = 94,
//     EADDRNOTAVAIL = 99,
//     ENETRESET = 102,
//     EISCONN = 106,
//     ENOTCONN = 107,
//     ETOOMANYREFS = 109,
//     EUSERS = 87,
//     EDQUOT = 122,
//     ESTALE = 116,
//     ENOTSUP = 95,
//     ENOMEDIUM = 123,
//     EILSEQ = 84,
//     EOVERFLOW = 75,
//     ECANCELED = 125,
//     ENOTRECOVERABLE = 131,
//     EOWNERDEAD = 130,
//     ESTRPIPE = 86,
// }

// pub struct ErrnoMessages<'a> {
//     message_map: HashMap<u32, &'a str>
// }

// impl<'a> ErrnoMessages<'a> {
//     fn new() -> Self {
//         let mut message_map = HashMap::new();
//         message_map.insert(0, "Success");
//         message_map.insert(1, "Not super-user");
//         message_map.insert(2, "No such file or directory");
//         message_map.insert(3, "No such process");
//         message_map.insert(4, "Interrupted system call");
//         message_map.insert(5, "I/O error");
//         message_map.insert(6, "No such device or address");
//         message_map.insert(7, "Arg list too long");
//         message_map.insert(8, "Exec format error");
//         message_map.insert(9, "Bad file number");
//         message_map.insert(10, "No children");
//         message_map.insert(11, "No more processes");
//         message_map.insert(12, "Not enough core");
//         message_map.insert(13, "Permission denied");
//         message_map.insert(14, "Bad address");
//         message_map.insert(15, "Block device required");
//         message_map.insert(16, "Mount device busy");
//         message_map.insert(17, "File exists");
//         message_map.insert(18, "Cross-device link");
//         message_map.insert(19, "No such device");
//         message_map.insert(20, "Not a directory");
//         message_map.insert(21, "Is a directory");
//         message_map.insert(22, "Invalid argument");
//         message_map.insert(23, "Too many open files in system");
//         message_map.insert(24, "Too many open files");
//         message_map.insert(25, "Not a typewriter");
//         message_map.insert(26, "Text file busy");
//         message_map.insert(27, "File too large");
//         message_map.insert(28, "No space left on device");
//         message_map.insert(29, "Illegal seek");
//         message_map.insert(30, "Read only file system");
//         message_map.insert(31, "Too many links");
//         message_map.insert(32, "Broken pipe");
//         message_map.insert(33, "Math arg out of domain of func");
//         message_map.insert(34, "Math result not representable");
//         message_map.insert(35, "File locking deadlock error");
//         message_map.insert(36, "File or path name too long");
//         message_map.insert(37, "No record locks available");
//         message_map.insert(38, "Function not implemented");
//         message_map.insert(39, "Directory not empty");
//         message_map.insert(40, "Too many symbolic links");
//         message_map.insert(42, "No message of desired type");
//         message_map.insert(43, "Identifier removed");
//         message_map.insert(44, "Channel number out of range");
//         message_map.insert(45, "Level 2 not synchronized");
//         message_map.insert(46, "Level 3 halted");
//         message_map.insert(47, "Level 3 reset");
//         message_map.insert(48, "Link number out of range");
//         message_map.insert(49, "Protocol driver not attached");
//         message_map.insert(50, "No CSI structure available");
//         message_map.insert(51, "Level 2 halted");
//         message_map.insert(52, "Invalid exchange");
//         message_map.insert(53, "Invalid request descriptor");
//         message_map.insert(54, "Exchange full");
//         message_map.insert(55, "No anode");
//         message_map.insert(56, "Invalid request code");
//         message_map.insert(57, "Invalid slot");
//         message_map.insert(59, "Bad font file fmt");
//         message_map.insert(60, "Device not a stream");
//         message_map.insert(61, "No data (for no delay io)");
//         message_map.insert(62, "Timer expired");
//         message_map.insert(63, "Out of streams resources");
//         message_map.insert(64, "Machine is not on the network");
//         message_map.insert(65, "Package not installed");
//         message_map.insert(66, "The object is remote");
//         message_map.insert(67, "The link has been severed");
//         message_map.insert(68, "Advertise error");
//         message_map.insert(69, "Srmount error");
//         message_map.insert(70, "Communication error on send");
//         message_map.insert(71, "Protocol error");
//         message_map.insert(72, "Multihop attempted");
//         message_map.insert(73, "Cross mount point (not really error)");
//         message_map.insert(74, "Trying to read unreadable message");
//         message_map.insert(75, "Value too large for defined data type");
//         message_map.insert(76, "Given log. name not unique");
//         message_map.insert(77, "f.d. invalid for this operation");
//         message_map.insert(78, "Remote address changed");
//         message_map.insert(79, "Can   access a needed shared lib");
//         message_map.insert(80, "Accessing a corrupted shared lib");
//         message_map.insert(81, ".lib section in a.out corrupted");
//         message_map.insert(82, "Attempting to link in too many libs");
//         message_map.insert(83, "Attempting to exec a shared library");
//         message_map.insert(84, "Illegal byte sequence");
//         message_map.insert(86, "Streams pipe error");
//         message_map.insert(87, "Too many users");
//         message_map.insert(88, "Socket operation on non-socket");
//         message_map.insert(89, "Destination address required");
//         message_map.insert(90, "Message too long");
//         message_map.insert(91, "Protocol wrong type for socket");
//         message_map.insert(92, "Protocol not available");
//         message_map.insert(93, "Unknown protocol");
//         message_map.insert(94, "Socket type not supported");
//         message_map.insert(95, "Not supported");
//         message_map.insert(96, "Protocol family not supported");
//         message_map.insert(97, "Address family not supported by protocol family");
//         message_map.insert(98, "Address already in use");
//         message_map.insert(99, "Address not available");
//         message_map.insert(100, "Network interface is not configured");
//         message_map.insert(101, "Network is unreachable");
//         message_map.insert(102, "Connection reset by network");
//         message_map.insert(103, "Connection aborted");
//         message_map.insert(104, "Connection reset by peer");
//         message_map.insert(105, "No buffer space available");
//         message_map.insert(106, "Socket is already connected");
//         message_map.insert(107, "Socket is not connected");
//         message_map.insert(108, "Can't send after socket shutdown");
//         message_map.insert(109, "Too many references");
//         message_map.insert(110, "Connection timed out");
//         message_map.insert(111, "Connection refused");
//         message_map.insert(112, "Host is down");
//         message_map.insert(113, "Host is unreachable");
//         message_map.insert(114, "Socket already connected");
//         message_map.insert(115, "Connection already in progress");
//         message_map.insert(116, "Stale file handle");
//         message_map.insert(122, "Quota exceeded");
//         message_map.insert(123, "No medium (in tape drive)");
//         message_map.insert(125, "Operation canceled");
//         message_map.insert(130, "Previous owner died");
//         message_map.insert(131, "State not recoverable");

//         ErrnoMessages {
//             message_map,
//         }
//     }
// }
