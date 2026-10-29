//! Keys: add_key, request_key and keyctl over the user, logon and keyring types, the special
//! keyrings, possession and permissions, and /proc/keys and /proc/key-users.

use wasixv2_abi::{GuestPtr, Signal};

use super::ipc::{errno, get, put};
use super::proc::ForkableMemory;
use super::*;

const THREAD: i32 = -1;
const PROCESS: i32 = -2;
const SESSION: i32 = -3;
const USER: i32 = -4;
const USER_SESSION: i32 = -5;
const GROUP: i32 = -6;

const GET_KEYRING_ID: i32 = 0;
const JOIN_SESSION_KEYRING: i32 = 1;
const UPDATE: i32 = 2;
const REVOKE: i32 = 3;
const SETPERM: i32 = 5;
const DESCRIBE: i32 = 6;
const CLEAR: i32 = 7;
const LINK: i32 = 8;
const UNLINK: i32 = 9;
const SEARCH: i32 = 10;
const READ: i32 = 11;
const SET_TIMEOUT: i32 = 15;
const INVALIDATE: i32 = 21;
const GET_PERSISTENT: i32 = 22;
const CAPABILITIES: i32 = 31;

/// One task's key calls. Strings go at 0x1000, 0x1100 and 0x1200, payloads at 0x2000, output
/// at 0x3000.
struct T<'a> {
    kernel: &'a Kernel,
    caller: Caller<'a>,
    memory: &'a FlatMemory,
}

impl<'a> T<'a> {
    fn new(kernel: &'a Kernel, task: TaskId, memory: &'a FlatMemory) -> Self {
        Self {
            kernel,
            caller: Caller::new(task, memory),
            memory,
        }
    }

    fn cstr(&self, at: u32, s: &str) -> Cstr {
        let mut bytes = s.as_bytes().to_vec();
        bytes.push(0);
        put(self.memory, at, &bytes);
        GuestPtr::new(at)
    }

    fn add(&self, kind: &str, desc: &str, payload: &[u8], ring: i32) -> SysResult<i32> {
        let (kind, desc) = (self.cstr(0x1000, kind), self.cstr(0x1100, desc));
        put(self.memory, 0x2000, payload);
        let data = wasixv2_abi::ConstVoidPtr::new(0x2000);
        let add = self
            .kernel
            .add_key(&self.caller, kind, desc, data, payload.len() as u32, ring);
        block_on(add)
    }

    fn request(&self, kind: &str, desc: &str, dest: i32) -> SysResult<i32> {
        let (kind, desc) = (self.cstr(0x1000, kind), self.cstr(0x1100, desc));
        let request = self
            .kernel
            .request_key(&self.caller, kind, desc, GuestPtr::NULL, dest);
        block_on(request)
    }

    fn keyctl(&self, op: i32, a2: u32, a3: u32, a4: u32, a5: u32) -> SysResult<i32> {
        block_on(self.kernel.keyctl(&self.caller, op, a2, a3, a4, a5))
    }

    fn id(&self, special: i32, create: bool) -> SysResult<i32> {
        self.keyctl(GET_KEYRING_ID, special as u32, create as u32, 0, 0)
    }

    fn describe(&self, key: i32) -> SysResult<String> {
        let n = self.keyctl(DESCRIBE, key as u32, 0x3000, 256, 0)? as usize;
        Ok(String::from_utf8(get(self.memory, 0x3000, n - 1)).unwrap())
    }

    fn read(&self, key: i32) -> SysResult<Vec<u8>> {
        let n = self.keyctl(READ, key as u32, 0x3000, 256, 0)? as usize;
        Ok(get(self.memory, 0x3000, n))
    }

    fn proc(&self, path: &str) -> String {
        let name = self.cstr(0x1200, path);
        let open = self
            .kernel
            .openat(&self.caller, Fd(-100), name, OpenFlags::O_RDONLY, 0);
        let fd = block_on(open).unwrap();
        let read = self
            .kernel
            .read(&self.caller, fd, UserSliceMut::new(0x4000, 4096));
        let n = block_on(read).unwrap();
        String::from_utf8(get(self.memory, 0x4000, n as usize)).unwrap()
    }
}

/// A process on a kernel with `/proc` mounted.
fn machine() -> (Kernel, TaskId, FlatMemory) {
    let (kernel, task, _) = setup(b"");
    let vfs = kernel.vfs().clone();
    let proc = vfs.mount(
        b"/proc",
        kernel.procfs(),
        wasixv2_vfs::MountFlags::default(),
        true,
    );
    block_on(proc).unwrap();
    (kernel, task, FlatMemory(RefCell::new(vec![0; 0x10000])))
}

/// A user key is added to a keyring, described, read, updated, and gone for good once revoked.
#[test]
fn user_keys_live_in_keyrings() {
    let (kernel, task, memory) = machine();
    let t = T::new(&kernel, task, &memory);
    let key = t.add("user", "secret", b"hunter2", SESSION).unwrap();
    assert!(key >= 3);
    assert_eq!(t.describe(key).unwrap(), "user;1;1;3f010000;secret");
    assert_eq!(t.read(key).unwrap(), b"hunter2");
    // The same type and description in the same keyring is updated in place.
    assert_eq!(t.add("user", "secret", b"swordfish", SESSION), Ok(key));
    assert_eq!(t.read(key).unwrap(), b"swordfish");
    put(&memory, 0x2000, b"new");
    assert_eq!(t.keyctl(UPDATE, key as u32, 0x2000, 3, 0), Ok(0));
    assert_eq!(t.read(key).unwrap(), b"new");
    assert_eq!(t.keyctl(REVOKE, key as u32, 0, 0, 0), Ok(0));
    assert_eq!(errno(t.read(key)), Errno::EKEYREVOKED);
    assert_eq!(errno(t.describe(key)), Errno::EKEYREVOKED);
    // Types: unknown, internal, empty payloads, payloads for keyrings, logon descriptions.
    assert_eq!(errno(t.add("nosuch", "x", b"1", SESSION)), Errno::ENODEV);
    assert_eq!(errno(t.add(".internal", "x", b"1", SESSION)), Errno::EPERM);
    assert_eq!(errno(t.add("user", "x", b"", SESSION)), Errno::EINVAL);
    assert_eq!(errno(t.add("keyring", "x", b"1", SESSION)), Errno::EINVAL);
    assert_eq!(
        errno(t.add("logon", "nocolon", b"1", SESSION)),
        Errno::EINVAL
    );
    let logon = t.add("logon", "svc:pw", b"1", SESSION).unwrap();
    assert_eq!(errno(t.read(logon)), Errno::EOPNOTSUPP);
}

/// The special keyrings: thread and process keyrings exist once asked for with `create`;
/// the session keyring is the user's session keyring until one is joined.
#[test]
fn special_keyrings() {
    let (kernel, task, memory) = machine();
    let t = T::new(&kernel, task, &memory);
    assert_eq!(errno(t.id(THREAD, false)), Errno::ENOKEY);
    let thread = t.id(THREAD, true).unwrap();
    assert_eq!(t.id(THREAD, false), Ok(thread));
    assert_eq!(errno(t.id(PROCESS, false)), Errno::ENOKEY);
    let process = t.id(PROCESS, true).unwrap();
    assert_ne!(process, thread);
    let user = t.id(USER, false).unwrap();
    let user_session = t.id(USER_SESSION, false).unwrap();
    assert!(t.describe(user).unwrap().ends_with(";_uid.1"));
    assert!(t.describe(user_session).unwrap().ends_with(";_uid_ses.1"));
    assert_eq!(t.id(SESSION, false), Ok(user_session));
    assert_eq!(errno(t.id(GROUP, false)), Errno::EINVAL);
    assert_eq!(errno(t.id(0, false)), Errno::EINVAL);
    let session = t.keyctl(JOIN_SESSION_KEYRING, 0, 0, 0, 0).unwrap();
    assert_eq!(t.id(SESSION, false), Ok(session));
    assert!(t.describe(session).unwrap().ends_with(";_ses"));
    let named = t.cstr(0x1100, "mysession");
    let joined = t
        .keyctl(JOIN_SESSION_KEYRING, named.addr(), 0, 0, 0)
        .unwrap();
    assert_eq!(t.id(SESSION, false), Ok(joined));
    assert_eq!(
        t.keyctl(JOIN_SESSION_KEYRING, named.addr(), 0, 0, 0),
        Ok(joined)
    );
}

/// request_key and KEYCTL_SEARCH find what the caller's keyrings hold and link it where asked;
/// what they do not hold is ENOKEY, as there is no upcall to make it.
#[test]
fn request_key_and_search_find_and_link() {
    let (kernel, task, memory) = machine();
    let t = T::new(&kernel, task, &memory);
    let ring = t.add("keyring", "ring", b"", SESSION).unwrap();
    let key = t.add("user", "inner", b"v", ring).unwrap();
    assert_eq!(t.request("user", "inner", 0), Ok(key));
    assert_eq!(errno(t.request("user", "missing", 0)), Errno::ENOKEY);
    let process = t.id(PROCESS, true).unwrap();
    assert_eq!(t.request("user", "inner", PROCESS), Ok(key));
    let n = t.keyctl(READ, process as u32, 0x3000, 64, 0).unwrap();
    assert_eq!(get(&memory, 0x3000, n as usize), key.to_le_bytes());
    let (kind, desc) = (t.cstr(0x1000, "user"), t.cstr(0x1100, "inner"));
    let session = t.id(SESSION, false).unwrap();
    assert_eq!(
        t.keyctl(SEARCH, session as u32, kind.addr(), desc.addr(), 0),
        Ok(key)
    );
    assert_eq!(t.keyctl(UNLINK, key as u32, process as u32, 0, 0), Ok(0));
    assert_eq!(
        errno(t.keyctl(UNLINK, key as u32, process as u32, 0, 0)),
        Errno::ENOENT
    );
    assert_eq!(t.keyctl(LINK, key as u32, process as u32, 0, 0), Ok(0));
    assert_eq!(t.keyctl(CLEAR, process as u32, 0, 0, 0), Ok(0));
    assert_eq!(t.read(process).unwrap(), b"");
    // A keyring never ends up inside itself.
    let inner = t.add("keyring", "inner-ring", b"", ring).unwrap();
    assert_eq!(
        errno(t.keyctl(LINK, ring as u32, inner as u32, 0, 0)),
        Errno::EDEADLK
    );
    assert_eq!(
        errno(t.keyctl(LINK, key as u32, key as u32, 0, 0)),
        Errno::ENOTDIR
    );
    // An invalidated key is gone.
    assert_eq!(t.keyctl(INVALIDATE, key as u32, 0, 0, 0), Ok(0));
    assert_eq!(errno(t.request("user", "inner", 0)), Errno::ENOKEY);
    assert_eq!(errno(t.read(key)), Errno::ENOKEY);
}

/// Permissions: the owner sets them; without the possessor's and owner's read, a read is
/// EACCES; unknown bits are EINVAL.
#[test]
fn permissions_decide() {
    let (kernel, task, memory) = machine();
    let t = T::new(&kernel, task, &memory);
    let key = t.add("user", "k", b"v", SESSION).unwrap();
    assert_eq!(
        errno(t.keyctl(SETPERM, key as u32, 0x4000_0000, 0, 0)),
        Errno::EINVAL
    );
    // Possessor: view and setattr; nothing else.
    assert_eq!(t.keyctl(SETPERM, key as u32, 0x2100_0000, 0, 0), Ok(0));
    assert_eq!(errno(t.read(key)), Errno::EACCES);
    assert_eq!(t.describe(key).unwrap(), "user;1;1;21000000;k");
    assert_eq!(t.keyctl(SET_TIMEOUT, key as u32, 90, 0, 0), Ok(0));
    let keys = t.proc("/proc/keys");
    let line = keys.lines().find(|l| l.ends_with(" k: 1")).expect(&keys);
    assert!(line.contains(" I--Q--- "), "{line}");
    assert!(
        line.contains("   1m 21000000     1     1 user      k: 1"),
        "{line}"
    );
    assert!(t.proc("/proc/key-users").starts_with("    1: "));
}

/// A child process shares the session keyring but starts without a process keyring; a thread
/// starts without a thread keyring.
#[test]
fn children_inherit_the_session_only() {
    let (kernel, task, flat) = machine();
    let memory = ForkableMemory(flat);
    let t = T::new(&kernel, task, &memory.0);
    let session = t.keyctl(JOIN_SESSION_KEYRING, 0, 0, 0, 0).unwrap();
    t.id(PROCESS, true).unwrap();
    t.id(THREAD, true).unwrap();
    let caller = Caller::new(task, &memory);
    let clone = kernel.clone(
        &caller,
        Signal::SIGCHLD.0 as u32,
        0,
        GuestPtrMut::NULL,
        0,
        GuestPtrMut::NULL,
    );
    let child = TaskId(block_on(clone).unwrap() as u32);
    let c = T::new(&kernel, child, &memory.0);
    assert_eq!(c.id(SESSION, false), Ok(session));
    assert_eq!(errno(c.id(PROCESS, false)), Errno::ENOKEY);
    assert_eq!(errno(c.id(THREAD, false)), Errno::ENOKEY);
}

/// The quota: 200 keys for a user other than root (EDQUOT past it).
#[test]
fn quota_limits_keys() {
    let (kernel, task, memory) = machine();
    let t = T::new(&kernel, task, &memory);
    let ring = t.add("keyring", "many", b"", SESSION).unwrap();
    let mut made = 0;
    let last = loop {
        match t.add("user", &format!("k{made}"), b"v", ring) {
            Ok(_) => made += 1,
            Err(err) => break err,
        }
    };
    assert_eq!(errno::<()>(Err(last)), Errno::EDQUOT);
    assert!(made < 200);
}

/// KEYCTL_CAPABILITIES says what this implementation has; what is not configured is
/// EOPNOTSUPP.
#[test]
fn capabilities_and_unconfigured_operations() {
    let (kernel, task, memory) = machine();
    let t = T::new(&kernel, task, &memory);
    assert_eq!(t.keyctl(CAPABILITIES, 0x3000, 2, 0, 0), Ok(2));
    assert_eq!(get(&memory, 0x3000, 2), [0xa1, 0]);
    assert_eq!(
        errno(t.keyctl(GET_PERSISTENT, 0, 0, 0, 0)),
        Errno::EOPNOTSUPP
    );
    assert_eq!(errno(t.keyctl(99, 0, 0, 0, 0)), Errno::EOPNOTSUPP);
}
