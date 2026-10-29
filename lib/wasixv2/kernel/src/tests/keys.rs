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
        self.request_with_callout(kind, desc, None, dest)
    }

    fn request_with_callout(
        &self,
        kind: &str,
        desc: &str,
        callout: Option<&str>,
        dest: i32,
    ) -> SysResult<i32> {
        let (kind, desc) = (self.cstr(0x1000, kind), self.cstr(0x1100, desc));
        let callout = callout.map_or(GuestPtr::NULL, |s| self.cstr(0x1200, s));
        block_on(
            self.kernel
                .request_key(&self.caller, kind, desc, callout, dest),
        )
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

    fn write_sysctl(&self, name: &str, value: &str) -> SysResult<u32> {
        let path = self.cstr(0x1200, &format!("/proc/sys/kernel/keys/{name}"));
        let fd =
            block_on(
                self.kernel
                    .openat(&self.caller, Fd(-100), path, OpenFlags::O_WRONLY, 0),
            )?;
        put(self.memory, 0x2000, value.as_bytes());
        let result = block_on(self.kernel.write(
            &self.caller,
            fd,
            UserSlice::new(0x2000, value.len() as u32),
        ));
        block_on(self.kernel.close(&self.caller, fd)).unwrap();
        result
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
    machine_as(1)
}

fn machine_as(uid: u32) -> (Kernel, TaskId, FlatMemory) {
    let host: Arc<dyn Host> = Arc::new(super::ptimer::RealHost(std::time::Instant::now()));
    let root = MemFs::new(Arc::new(HostClock(host.clone())), 0o755, 1, 1);
    let kernel = Kernel::new(host, Arc::new(root)).with_credentials(Credentials::new(uid, uid));
    let stdin = Arc::new(Input(Mutex::new(Vec::new())));
    let (kernel, task, _) = spawn_on(kernel, stdin, &[], &[]);
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

/// Linux security/keys/request_key.c `complete_request_key` caches the failed construction,
/// even when the usermode helper is absent; keyctl_read_key must not read a negative payload.
#[test]
fn failed_request_key_construction_caches_a_negative_key() {
    let (kernel, task, memory) = machine();
    let t = T::new(&kernel, task, &memory);
    let ring = t.id(PROCESS, true).unwrap();
    assert_eq!(errno(t.request("user", "missing", PROCESS)), Errno::ENOKEY);
    assert!(t.read(ring).unwrap().is_empty());
    assert_eq!(
        errno(t.request_with_callout("user", "missing", Some(""), PROCESS)),
        Errno::ENOENT
    );
    let ids = t.read(ring).unwrap();
    assert_eq!(ids.len(), 4);
    let key = i32::from_le_bytes(ids.try_into().unwrap());
    assert!(t.describe(key).unwrap().ends_with(";missing"));
    assert_eq!(errno(t.read(key)), Errno::ENOKEY);
    assert_eq!(
        errno(t.keyctl(LINK, key as u32, ring as u32, 0, 0)),
        Errno::ENOKEY
    );
    assert_eq!(
        errno(t.keyctl(UPDATE, key as u32, 0x2000, 1, 0)),
        Errno::ENOKEY
    );
    assert_eq!(
        errno(t.request_with_callout("user", "missing", Some("again"), PROCESS)),
        Errno::ENOKEY
    );
    assert_eq!(t.read(ring).unwrap(), key.to_le_bytes());
    let keys = t.proc("/proc/keys");
    let line = keys.lines().find(|l| l.contains(" missing")).unwrap();
    assert!(line.contains(" I--Q-N- "), "{line}");
    // key_create_or_update may update a negative key, making it positive and permanent.
    assert_eq!(t.add("user", "missing", b"present", ring), Ok(key));
    assert_eq!(t.read(key).unwrap(), b"present");
    assert_eq!(t.request("user", "missing", 0), Ok(key));
}

#[test]
fn failed_request_key_construction_expires_and_obeys_the_default_destination() {
    let (kernel, task, memory) = machine();
    let t = T::new(&kernel, task, &memory);
    let session = t.keyctl(JOIN_SESSION_KEYRING, 0, 0, 0, 0).unwrap();
    assert_eq!(t.keyctl(14, 3, 0, 0, 0), Ok(0)); // KEYCTL_SET_REQKEY_KEYRING: session.
    assert_eq!(
        errno(t.request_with_callout("user", "cached", Some("x"), 0)),
        Errno::ENOENT
    );
    let old = t.read(session).unwrap();
    kernel
        .time
        .settime(kernel.host.realtime() + Duration::from_secs(61))
        .unwrap();
    assert_eq!(
        errno(t.request_with_callout("user", "cached", Some("x"), 0)),
        Errno::ENOENT
    );
    assert_ne!(t.read(session).unwrap(), old);
    // construct_get_dest_keyring requires WRITE, even for a joined session keyring.
    assert_eq!(t.keyctl(CLEAR, session as u32, 0, 0, 0), Ok(0));
    assert_eq!(t.keyctl(SETPERM, session as u32, 0x0b00_0000, 0, 0), Ok(0));
    assert_eq!(
        errno(t.request_with_callout("user", "denied", Some("x"), 0)),
        Errno::EACCES
    );
    assert!(t.read(session).unwrap().is_empty());
}

/// security/keys/keyring.c search_nested_keyrings does not report the state of an
/// unrelated traversal ring; an expired ring can still contain a live matching key.
#[test]
fn unrelated_expired_keyrings_do_not_replace_a_search_miss() {
    let (kernel, task, memory) = machine();
    let t = T::new(&kernel, task, &memory);
    let revoked = t.add("keyring", "revoked", b"", THREAD).unwrap();
    assert_eq!(t.keyctl(REVOKE, revoked as u32, 0, 0, 0), Ok(0));
    let expired = t.add("keyring", "expired", b"", THREAD).unwrap();
    let live = t.add("user", "reachable", b"payload", expired).unwrap();
    assert_eq!(t.keyctl(SET_TIMEOUT, expired as u32, 1, 0, 0), Ok(0));
    kernel
        .time
        .settime(kernel.host.realtime() + Duration::from_secs(2))
        .unwrap();
    assert_eq!(errno(t.request("keyring", "missing", 0)), Errno::ENOKEY);
    assert_eq!(t.request("user", "reachable", 0), Ok(live));
    // Explicit destination lookup still validates the destination's own state.
    assert_eq!(
        errno(t.request("keyring", "missing", revoked)),
        Errno::EKEYREVOKED
    );
    assert_eq!(
        errno(t.request("keyring", "missing", expired)),
        Errno::EKEYEXPIRED
    );
}

/// security/keys/keyctl.c checks malformed REJECT/INSTANTIATE arguments before authority;
/// neither the caller's credentials nor root grant a request-key authorization token.
#[test]
fn request_key_authority_operations_validate_arguments_without_authority() {
    let (kernel, task, memory) = machine();
    let t = T::new(&kernel, task, &memory);
    assert_eq!(errno(t.keyctl(19, 0, 0, 0, 0)), Errno::EINVAL); // REJECT
    for error in [4095, 512, 513, 514, 516] {
        assert_eq!(errno(t.keyctl(19, 0, 0, error, 0)), Errno::EINVAL);
    }
    assert_eq!(
        errno(t.keyctl(19, 0, 0, Errno::ENOKEY.0 as u32, 0)),
        Errno::EPERM
    );
    assert_eq!(errno(t.keyctl(13, 0, 0, 0, 0)), Errno::EPERM); // NEGATE
    assert_eq!(
        errno(t.keyctl(12, 0, 0x2000, 1024 * 1024, 0)),
        Errno::EINVAL
    );
    assert_eq!(errno(t.keyctl(12, 0, 0x2000, 1, 0)), Errno::EPERM);
    assert_eq!(errno(t.keyctl(20, 0, 0x2000, 1025, 0)), Errno::EINVAL); // INSTANTIATE_IOV
    assert_eq!(errno(t.keyctl(20, 0, 0xffff_ff00, 1, 0)), Errno::EFAULT);
    assert_eq!(errno(t.keyctl(20, 0, 0, 1025, 0)), Errno::EPERM);
    assert_eq!(t.keyctl(16, 0, 0, 0, 0), Ok(0)); // ASSUME_AUTHORITY
    assert_eq!(errno(t.keyctl(16, u32::MAX, 0, 0, 0)), Errno::EINVAL);
    assert_eq!(errno(t.keyctl(16, 123, 0, 0, 0)), Errno::ENOKEY);
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
    assert_eq!(get(&memory, 0x3000, 2), [0xa1, 4]);
    assert_eq!(
        errno(t.keyctl(GET_PERSISTENT, 0, 0, 0, 0)),
        Errno::EOPNOTSUPP
    );
    assert_eq!(errno(t.keyctl(99, 0, 0, 0, 0)), Errno::EOPNOTSUPP);
}

/// Linux security/keys/sysctl.c exposes the configured tunables as global 0644 integers.
#[test]
fn key_sysctls_validate_values_and_bound_key_quotas() {
    let (kernel, task, memory) = machine_as(0);
    let t = T::new(&kernel, task, &memory);
    for (name, initial) in [
        ("maxkeys", 200),
        ("maxbytes", 20000),
        ("root_maxkeys", 1_000_000),
        ("root_maxbytes", 25_000_000),
        ("gc_delay", 300),
    ] {
        let path = format!("/proc/sys/kernel/keys/{name}");
        assert_eq!(t.proc(&path), format!("{initial}\n"));
        for value in ["-1", "2147483648", "x"] {
            assert_eq!(errno(t.write_sysctl(name, value)), Errno::EINVAL);
        }
        if name != "gc_delay" {
            assert_eq!(errno(t.write_sysctl(name, "0")), Errno::EINVAL);
        }
        assert_eq!(t.proc(&path), format!("{initial}\n"));
    }
    t.id(PROCESS, true).unwrap();
    t.id(USER, false).unwrap();
    t.id(USER_SESSION, false).unwrap();
    assert_eq!(t.write_sysctl("root_maxkeys", "4"), Ok(1));
    t.add("user", "a", b"x", PROCESS).unwrap();
    assert_eq!(errno(t.add("user", "b", b"x", PROCESS)), Errno::EDQUOT);
    assert!(t.proc("/proc/key-users").contains(" 4/4 "));
    assert_eq!(t.write_sysctl("root_maxkeys", "5"), Ok(1));
    let key = t.add("user", "b", b"payload", PROCESS).unwrap();
    // Lowering a byte quota does not prevent shrinking a payload already over the quota.
    assert_eq!(t.write_sysctl("root_maxbytes", "1"), Ok(1));
    assert_eq!(t.add("user", "b", b"x", PROCESS), Ok(key));
    assert_eq!(errno(t.add("user", "b", b"longer", PROCESS)), Errno::EDQUOT);
    assert_eq!(t.write_sysctl("maxkeys", "1"), Ok(1));
    assert_eq!(block_on(kernel.setuid(&t.caller, 1)), Ok(0));
    t.add("user", "owned-by-user", b"x", PROCESS).unwrap();
    assert_eq!(
        errno(t.add("user", "second-user-key", b"x", PROCESS)),
        Errno::EDQUOT
    );
    assert_eq!(errno(t.write_sysctl("maxkeys", "2")), Errno::EACCES);
}

/// security/keys/gc.c `key_garbage_collector` and keyring.c `keyring_gc` remove dead links
/// after the tunable grace period, freeing their quota. CLOCK_REALTIME sets must rearm it.
#[test]
fn key_sysctls_gc_delay_controls_expired_revoked_and_invalidated_links() {
    let (kernel, task, memory) = machine_as(0);
    let t = T::new(&kernel, task, &memory);
    t.id(PROCESS, true).unwrap();
    assert_eq!(t.write_sysctl("gc_delay", "10"), Ok(2));
    let key = t.add("user", "expiring", b"x", PROCESS).unwrap();
    assert_eq!(t.keyctl(SET_TIMEOUT, key as u32, 1, 0, 0), Ok(0));
    kernel
        .time
        .settime(kernel.host.realtime() + Duration::from_secs(2))
        .unwrap();
    kernel.clock_was_set();
    assert_eq!(t.read(PROCESS).unwrap(), key.to_le_bytes());
    assert_eq!(t.write_sysctl("gc_delay", "0"), Ok(1));
    assert!(t.read(PROCESS).unwrap().is_empty());
    assert_eq!(errno(t.describe(key)), Errno::ENOKEY);
    let key = t.add("user", "revoked", b"x", PROCESS).unwrap();
    assert_eq!(t.keyctl(REVOKE, key as u32, 0, 0, 0), Ok(0));
    // Re-run a clock-set notification synchronously, so this does not race the driver.
    kernel.clock_was_set();
    assert!(t.read(PROCESS).unwrap().is_empty());
    assert_eq!(errno(t.describe(key)), Errno::ENOKEY);
    let key = t.add("user", "invalidated", b"x", PROCESS).unwrap();
    assert_eq!(t.keyctl(INVALIDATE, key as u32, 0, 0, 0), Ok(0));
    kernel.clock_was_set();
    assert!(t.read(PROCESS).unwrap().is_empty());
}

#[test]
fn key_watches_report_mutations_ids_and_removals_through_notification_pipes() {
    use wasixv2_abi::{GuestLayout, IoctlRequest, KeyNotification};
    const WATCH: i32 = 32;
    let (kernel, task, memory) = machine();
    let t = T::new(&kernel, task, &memory);
    let ring = t.id(SESSION, true).unwrap();
    let keep = t.id(PROCESS, true).unwrap();
    let (r, _) = super::ipc::pipe(
        &kernel,
        &t.caller,
        OpenFlags::O_NOTIFICATION_PIPE | OpenFlags::O_NONBLOCK,
    );
    assert_eq!(
        block_on(kernel.ioctl(&t.caller, r, IoctlRequest::IOC_WATCH_QUEUE_SET_SIZE, 64)),
        Ok(0)
    );
    assert_eq!(t.keyctl(WATCH, ring as u32, r.0 as u32, 0x5a, 0), Ok(0));
    let key = t.add("user", "watched", b"one", ring).unwrap();
    assert_eq!(t.keyctl(WATCH, key as u32, r.0 as u32, 3, 0), Ok(0));
    assert_eq!(
        t.keyctl(WATCH, key as u32, r.0 as u32, 4, 0),
        Err(SysErr::Errno(Errno::EBUSY))
    );
    put(&memory, 0x2000, b"two");
    assert_eq!(t.keyctl(UPDATE, key as u32, 0x2000, 3, 0), Ok(0));
    assert_eq!(t.add("user", "watched", b"three", ring), Ok(key));
    assert_eq!(t.keyctl(SET_TIMEOUT, key as u32, 10, 0, 0), Ok(0));
    assert_eq!(t.keyctl(LINK, key as u32, keep as u32, 0, 0), Ok(0));
    assert_eq!(t.keyctl(UNLINK, key as u32, ring as u32, 0, 0), Ok(0));
    assert_eq!(t.keyctl(CLEAR, ring as u32, 0, 0, 0), Ok(0));
    assert_eq!(t.keyctl(WATCH, key as u32, r.0 as u32, u32::MAX, 0), Ok(0));
    let n = block_on(kernel.read(&t.caller, r, UserSliceMut::new(0x4000, 256))).unwrap();
    assert_eq!(n, 7 * 16);
    let raw = get(&memory, 0x4000, n as usize);
    let events: Vec<_> = raw
        .chunks_exact(16)
        .map(|b| {
            let e = KeyNotification::read_from(b).unwrap();
            (e.watch.type_subtype, e.watch.info >> 8, e.key_id, e.aux)
        })
        .collect();
    assert_eq!(
        events[..6],
        [
            (1 | 2 << 24, 0x5a, ring as u32, key as u32),
            (1 | 1 << 24, 3, key as u32, 0),
            (1 | 1 << 24, 3, key as u32, 0),
            (1 | 7 << 24, 3, key as u32, 0),
            (1 | 3 << 24, 0x5a, ring as u32, key as u32),
            (1 | 4 << 24, 0x5a, ring as u32, 0),
        ]
    );
    assert_eq!(events[6].0, 0); // WATCH_META_REMOVAL_NOTIFICATION, object serial in u64 body.
    assert_eq!(events[6].1, 3);
    assert_eq!(
        t.keyctl(WATCH, key as u32, r.0 as u32, u32::MAX, 0),
        Err(SysErr::Errno(Errno::EBADSLT))
    );
    assert_eq!(t.keyctl(WATCH, key as u32, r.0 as u32, 9, 0), Ok(0));
    assert_eq!(t.keyctl(REVOKE, key as u32, 0, 0, 0), Ok(0));
    assert_eq!(
        block_on(kernel.read(&t.caller, r, UserSliceMut::new(0x4000, 16))),
        Ok(16)
    );
    let revoked = KeyNotification::read_from(&get(&memory, 0x4000, 16)).unwrap();
    assert_eq!(revoked.watch.type_subtype, 1 | 5 << 24);
    let other = t.add("user", "invalidated", b"x", ring).unwrap();
    assert_eq!(t.keyctl(WATCH, other as u32, r.0 as u32, 10, 0), Ok(0));
    assert_eq!(t.keyctl(INVALIDATE, other as u32, 0, 0, 0), Ok(0));
    let n = block_on(kernel.read(&t.caller, r, UserSliceMut::new(0x4000, 64))).unwrap();
    assert!(
        get(&memory, 0x4000, n as usize)
            .chunks_exact(16)
            .any(|b| KeyNotification::read_from(b).unwrap().watch.type_subtype == 1 | 6 << 24)
    );
}

#[test]
fn key_watch_checks_view_permission_and_linux_error_precedence() {
    let (kernel, task, memory) = machine();
    let t = T::new(&kernel, task, &memory);
    let (r, _) = super::ipc::pipe(&kernel, &t.caller, OpenFlags::O_NOTIFICATION_PIPE);
    let key = t.add("user", "no-view", b"secret", SESSION).unwrap();
    assert_eq!(
        t.keyctl(32, key as u32, r.0 as u32, 256, 0),
        Err(SysErr::Errno(Errno::EINVAL))
    );
    assert_eq!(
        t.keyctl(32, key as u32, u32::MAX, 1, 0),
        Err(SysErr::Errno(Errno::EINVAL))
    );
    // Keep SETATTR, WRITE and SEARCH, but no VIEW even when possessed.
    assert_eq!(t.keyctl(SETPERM, key as u32, 0x3e3e0000, 0, 0), Ok(0));
    assert_eq!(
        t.keyctl(32, key as u32, r.0 as u32, 1, 0),
        Err(SysErr::Errno(Errno::EACCES))
    );
    assert_eq!(
        t.keyctl(32, key as u32, u32::MAX, u32::MAX, 0),
        Err(SysErr::Errno(Errno::EACCES))
    );
}

/// kernel/cred.c `copy_creds` and security/keys/process_keys.c
/// `install_session_keyring_to_cred`: keyring references belong to each task's cred.
#[test]
fn thread_credentials_own_keyrings_and_join_session_affects_only_the_caller() {
    let (kernel, leader, memory) = machine();
    let main = T::new(&kernel, leader, &memory);
    let thread_ring = main.id(THREAD, true).unwrap();
    let process_ring = main.id(PROCESS, true).unwrap();
    let old_session = main.keyctl(JOIN_SESSION_KEYRING, 0, 0, 0, 0).unwrap();
    main.add("user", "private", b"secret", THREAD).unwrap();
    let child = TaskId(
        block_on(kernel.thread_spawn(
            &main.caller,
            0x10f00,
            0,
            GuestPtrMut::NULL,
            0,
            GuestPtrMut::NULL,
        ))
        .unwrap() as u32,
    );
    let worker = T::new(&kernel, child, &memory);
    let child_thread_ring = worker.id(THREAD, false).unwrap();
    assert_ne!(child_thread_ring, thread_ring);
    assert_eq!(worker.read(child_thread_ring).unwrap(), b"");
    assert_eq!(worker.id(PROCESS, false), Ok(process_ring));
    assert_eq!(worker.id(SESSION, false), Ok(old_session));
    let joined = worker.keyctl(JOIN_SESSION_KEYRING, 0, 0, 0, 0).unwrap();
    assert_ne!(joined, old_session);
    assert_eq!(main.id(SESSION, false), Ok(old_session));
    assert_eq!(worker.id(SESSION, false), Ok(joined));
    assert!(kernel.task_cred(child).keys.thread.is_some());
    // A fork inherits the calling thread's session, and neither thread/process keyring.
    let fork_memory = ForkableMemory(FlatMemory(RefCell::new(vec![0; 65536])));
    let pid = block_on(kernel.clone(
        &Caller::new(child, &fork_memory),
        Signal::SIGCHLD.0 as u32,
        0,
        GuestPtrMut::NULL,
        0,
        GuestPtrMut::NULL,
    ))
    .unwrap();
    let fork = T::new(&kernel, TaskId(pid as u32), &memory);
    assert_eq!(fork.id(SESSION, false), Ok(joined));
    assert_eq!(errno(fork.id(THREAD, false)), Errno::ENOKEY);
    assert_eq!(errno(fork.id(PROCESS, false)), Errno::ENOKEY);
}
