//! Processes, threads, signals and terminals.

use super::{Bool, Errno, Ptr, Size, Timestamp, layout, newtype_layout};

/// `signal` (`u8`). 1..=31 are Linux's generic numbers; v1 has no real-time signals.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Signal(pub u8);

impl Signal {
    pub const NONE: Self = Self(0);
    pub const SIGHUP: Self = Self(1);
    pub const SIGINT: Self = Self(2);
    pub const SIGQUIT: Self = Self(3);
    pub const SIGKILL: Self = Self(9);
    pub const SIGALRM: Self = Self(14);
    pub const SIGTERM: Self = Self(15);
    pub const SIGCHLD: Self = Self(17);
    pub const SIGSTOP: Self = Self(19);
    pub const SIGSYS: Self = Self(31);

    /// The Linux signal, or `None` outside 1..=31.
    pub fn to_linux(self) -> Option<wasixv2_abi::Signal> {
        (1..=31)
            .contains(&self.0)
            .then_some(wasixv2_abi::Signal(i32::from(self.0)))
    }

    /// The v1 signal for a Linux one, or `None` for one v1 cannot name (real-time signals).
    pub fn from_linux(sig: i32) -> Option<Self> {
        (1..=31).contains(&sig).then_some(Self(sig as u8))
    }
}

/// `disposition` (`u8`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Disposition(pub u8);

impl Disposition {
    pub const DEFAULT: Self = Self(0);
    pub const IGNORE: Self = Self(1);
}

/// `proc_spawn_fd_op_name` (`u8`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcSpawnFdOpName(pub u8);

impl ProcSpawnFdOpName {
    pub const CLOSE: Self = Self(0);
    pub const DUP2: Self = Self(1);
    pub const OPEN: Self = Self(2);
    pub const CHDIR: Self = Self(3);
    pub const FCHDIR: Self = Self(4);
}

/// `join_flags` (`u32`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JoinFlags(pub u32);

impl JoinFlags {
    pub const NON_BLOCKING: Self = Self(1 << 0);
    pub const WAKE_STOPPED: Self = Self(1 << 1);
}

/// `join_status_type` (`u8`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JoinStatusType(pub u8);

impl JoinStatusType {
    pub const NOTHING: Self = Self(0);
    pub const EXIT_NORMAL: Self = Self(1);
    pub const EXIT_SIGNAL: Self = Self(2);
    pub const STOPPED: Self = Self(3);
}

/// `option_tag` (`u8`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OptionTag(pub u8);

impl OptionTag {
    pub const NONE: Self = Self(0);
    pub const SOME: Self = Self(1);
}

newtype_layout!(
    Signal(u8),
    Disposition(u8),
    ProcSpawnFdOpName(u8),
    JoinFlags(u32),
    JoinStatusType(u8),
    OptionTag(u8),
);

layout! {
    /// `option_pid`.
    pub struct OptionPid (size 8, align 4) {
        pub tag: OptionTag = 0,
        pub pid: u32 = 4,
    }
}

layout! {
    /// `option_timestamp`.
    pub struct OptionTimestamp (size 16, align 8) {
        pub tag: OptionTag = 0,
        pub timestamp: Timestamp = 8,
    }
}

layout! {
    /// `join_status`: `tag`, then for `EXIT_NORMAL` the exit code as an errno-sized `u16`, for
    /// `EXIT_SIGNAL` that code and the signal at 4, for `STOPPED` the signal at 2.
    pub struct JoinStatus (size 6, align 2) {
        pub tag: JoinStatusType = 0,
        pub code: Errno = 2,
        pub signal: Signal = 4,
    }
}

layout! {
    /// `signal_disposition`.
    pub struct SignalDisposition (size 2, align 1) {
        pub sig: Signal = 0,
        pub disp: Disposition = 1,
    }
}

layout! {
    /// `thread_start`: what `thread_spawn_v2` gets; the host reads only `tls_base`, the guest
    /// trampoline the rest.
    pub struct ThreadStart (size 64, align 4) {
        pub stack: Ptr = 0,
        pub tls_base: Ptr = 4,
        pub start_funct: Ptr = 8,
        pub start_args: Ptr = 12,
        pub stack_size: Size = 56,
        pub guard_size: Size = 60,
    }
}

layout! {
    /// `proc_spawn_fd_op`.
    pub struct ProcSpawnFdOp (size 56, align 8) {
        pub cmd: ProcSpawnFdOpName = 0,
        pub fd: u32 = 4,
        pub src_fd: u32 = 8,
        pub path: Ptr = 12,
        pub path_len: Size = 16,
        pub dirflags: u32 = 20,
        pub oflags: u16 = 24,
        pub fs_rights_base: u64 = 32,
        pub fs_rights_inheriting: u64 = 40,
        pub fdflags: u16 = 48,
        pub fdflagsext: u16 = 50,
    }
}

layout! {
    /// `tty`.
    pub struct Tty (size 24, align 4) {
        pub cols: u32 = 0,
        pub rows: u32 = 4,
        pub width: u32 = 8,
        pub height: u32 = 12,
        pub stdin_tty: Bool = 16,
        pub stdout_tty: Bool = 17,
        pub stderr_tty: Bool = 18,
        pub echo: Bool = 19,
        pub line_buffered: Bool = 20,
        pub line_feeds: Bool = 21,
    }
}

layout! {
    /// `stack_snapshot`: identical to v2's `stack-snapshot`.
    pub struct StackSnapshot (size 24, align 8) {
        pub user: u64 = 0,
        pub hash: [u64; 2] = 8,
    }
}

/// `exitcode` (`u32`); Linux keeps its low 8 bits.
pub type ExitCode = u32;
