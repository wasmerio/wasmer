//! The kernel log (`kernel/printk/printk.c`): a ring of records the kernel's messages go to,
//! read with `syslog(2)`, `/proc/kmsg` (which consumes) and `/dev/kmsg` (one record per read,
//! where userspace also writes them).
//!
//! A record holds its sequence number, facility, level, time since boot and text. The ring
//! keeps the newest records within `log_buf_len` bytes, counting each record's text and its
//! header as the printk ringbuffer would hold them. There is no console: no message is
//! printed anywhere, so the console levels are kept but change nothing.

use std::{
    collections::VecDeque,
    fmt::Write as _,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI32, AtomicU8, Ordering},
    },
    time::Duration,
};

use wasixv2_abi::{Buf, OpenFlags};
use wasixv2_host::Host;
use wasixv2_syscalls::{Caller, Errno, LocalFuture, SysErr, SysResult};
use wasixv2_vfs::{Cap, File};
use wasixv2_wait::{PollMask, PollTable, Pollable, WaitQueue, poll_wait, wait_event};

use crate::{
    Kernel,
    file::{FileObject, Io, SeekBase},
    sync::MutexExt,
};

/// `CONFIG_LOG_BUF_SHIFT` 17: the ring's size, `log_buf_len`.
pub(crate) const LOG_BUF_LEN: usize = 1 << 17;
/// What a record costs besides its text: its descriptor and info (`struct prb_desc`,
/// `struct printk_info`).
const RECORD_OVERHEAD: usize = 88;
/// `PRINTKRB_RECORD_MAX`: the longest record `/dev/kmsg` takes.
const RECORD_MAX: usize = 1024 - 32;
/// `LOG_KERN` and `LOG_USER`.
const LOG_KERN: u8 = 0;
const LOG_USER: u8 = 1;
/// `MESSAGE_LOGLEVEL_DEFAULT`, `CONSOLE_LOGLEVEL_MIN` and `CONSOLE_LOGLEVEL_DEFAULT`.
const DEFAULT_MESSAGE_LOGLEVEL: i32 = 4;
const MINIMUM_CONSOLE_LOGLEVEL: i32 = 1;
const DEFAULT_CONSOLE_LOGLEVEL: i32 = 7;
/// `LOGLEVEL_DEFAULT`: no saved console level.
const LOGLEVEL_DEFAULT: i32 = -1;
/// `DEFAULT_RATELIMIT_INTERVAL` and `DEFAULT_RATELIMIT_BURST`, for `/dev/kmsg` writers.
const RATELIMIT_INTERVAL: Duration = Duration::from_secs(5);
const RATELIMIT_BURST: u32 = 10;

/// The `syslog(2)` actions (`SYSLOG_ACTION_*`).
const SYSLOG_ACTION_CLOSE: i32 = 0;
const SYSLOG_ACTION_OPEN: i32 = 1;
const SYSLOG_ACTION_READ: i32 = 2;
const SYSLOG_ACTION_READ_ALL: i32 = 3;
const SYSLOG_ACTION_READ_CLEAR: i32 = 4;
const SYSLOG_ACTION_CLEAR: i32 = 5;
const SYSLOG_ACTION_CONSOLE_OFF: i32 = 6;
const SYSLOG_ACTION_CONSOLE_ON: i32 = 7;
const SYSLOG_ACTION_CONSOLE_LEVEL: i32 = 8;
const SYSLOG_ACTION_SIZE_UNREAD: i32 = 9;
const SYSLOG_ACTION_SIZE_BUFFER: i32 = 10;

/// Message levels (`KERN_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Emerg = 0,
    Alert = 1,
    Crit = 2,
    Err = 3,
    Warning = 4,
    Notice = 5,
    Info = 6,
    Debug = 7,
}

/// Where a `do_syslog` call comes from (`SYSLOG_FROM_READER`, `SYSLOG_FROM_PROC`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
    Reader,
    Proc,
}

/// `printk_devkmsg`: what `/dev/kmsg` does with writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Devkmsg {
    On = 0,
    Off = 1,
    Ratelimit = 2,
}

struct Record {
    seq: u64,
    facility: u8,
    level: u8,
    /// Time since boot (`ts_nsec`).
    ts: Duration,
    text: Vec<u8>,
}

impl Record {
    fn cost(&self) -> usize {
        self.text.len() + RECORD_OVERHEAD
    }

    /// `info_print_prefix`: `<prio>` for syslog, then the time.
    fn prefix(&self, syslog: bool) -> String {
        let mut out = String::new();
        if syslog {
            let _ = write!(out, "<{}>", (self.facility << 3) | self.level);
        }
        let _ = write!(
            out,
            "[{:5}.{:06}] ",
            self.ts.as_secs(),
            self.ts.subsec_micros()
        );
        out
    }

    /// `record_print_text`: every line of the text with the prefix, each ending in a newline.
    fn print_text(&self, syslog: bool) -> Vec<u8> {
        let prefix = self.prefix(syslog);
        let mut out = Vec::new();
        for line in self.text.split(|&b| b == b'\n') {
            out.extend_from_slice(prefix.as_bytes());
            out.extend_from_slice(line);
            out.push(b'\n');
        }
        out
    }

    /// The record as `/dev/kmsg` reads it (`info_print_ext_header`, `msg_print_ext_body`):
    /// `prio,seq,usec,flags;` and the text with unprintable bytes escaped.
    fn ext_text(&self) -> Vec<u8> {
        let mut out = format!(
            "{},{},{},-;",
            (self.facility << 3) | self.level,
            self.seq,
            self.ts.as_micros()
        );
        for &c in &self.text {
            if !(b' '..127).contains(&c) || c == b'\\' {
                let _ = write!(out, "\\x{c:02x}");
            } else {
                out.push(c as char);
            }
        }
        out.push('\n');
        out.into_bytes()
    }
}

#[derive(Default)]
struct Log {
    records: VecDeque<Record>,
    bytes: usize,
    /// The sequence number the next record gets (`prb_next_seq`).
    next_seq: u64,
    /// Where `SYSLOG_ACTION_READ` and `/proc/kmsg` go on (`syslog_seq`), and how much of that
    /// record they read already (`syslog_partial`).
    syslog_seq: u64,
    syslog_partial: usize,
    /// Where `SYSLOG_ACTION_READ_ALL` starts (`clear_seq`).
    clear_seq: u64,
}

impl Log {
    fn first_seq(&self) -> u64 {
        self.records.front().map_or(self.next_seq, |r| r.seq)
    }

    /// The record `seq`, or the first one after it that is still there.
    fn from(&self, seq: u64) -> impl Iterator<Item = &Record> {
        let skip = seq.saturating_sub(self.first_seq()) as usize;
        self.records.iter().skip(skip)
    }
}

/// The kernel log and its tunables.
pub(crate) struct Printk {
    log: Mutex<Log>,
    /// `log_wait`.
    wait: WaitQueue,
    host: Arc<dyn Host>,
    /// `kernel.dmesg_restrict`.
    dmesg_restrict: AtomicBool,
    /// `kernel.printk`: console, default message, minimum console and default console levels.
    console_loglevel: AtomicI32,
    default_message_loglevel: AtomicI32,
    minimum_console_loglevel: AtomicI32,
    default_console_loglevel: AtomicI32,
    /// What `SYSLOG_ACTION_CONSOLE_OFF` saved.
    saved_console_loglevel: AtomicI32,
    /// `kernel.printk_devkmsg`.
    devkmsg: AtomicU8,
}

impl Printk {
    pub(crate) fn new(host: Arc<dyn Host>) -> Self {
        Self {
            log: Mutex::default(),
            wait: WaitQueue::new(),
            host,
            dmesg_restrict: AtomicBool::new(false),
            console_loglevel: AtomicI32::new(DEFAULT_CONSOLE_LOGLEVEL),
            default_message_loglevel: AtomicI32::new(DEFAULT_MESSAGE_LOGLEVEL),
            minimum_console_loglevel: AtomicI32::new(MINIMUM_CONSOLE_LOGLEVEL),
            default_console_loglevel: AtomicI32::new(DEFAULT_CONSOLE_LOGLEVEL),
            saved_console_loglevel: AtomicI32::new(LOGLEVEL_DEFAULT),
            devkmsg: AtomicU8::new(Devkmsg::Ratelimit as u8),
        }
    }

    /// `printk` at `level` from the kernel (`LOG_KERN`).
    pub(crate) fn log(&self, level: Level, text: &str) {
        self.emit(LOG_KERN, level as u8, text.as_bytes());
    }

    /// `vprintk_store`: a record of `text`, without its trailing newline, which may push the
    /// oldest ones out of the ring.
    fn emit(&self, facility: u8, level: u8, text: &[u8]) {
        let text = text.strip_suffix(b"\n").unwrap_or(text);
        let ts = self.host.monotonic();
        let mut log = self.log.lock_or_recover();
        let record = Record {
            seq: log.next_seq,
            facility,
            level: level & 7,
            ts,
            text: text.to_vec(),
        };
        log.next_seq += 1;
        log.bytes += record.cost();
        log.records.push_back(record);
        while log.bytes > LOG_BUF_LEN {
            let old = log.records.pop_front().expect("over the size with records");
            log.bytes -= old.cost();
        }
        drop(log);
        self.wait.wake(PollMask::READABLE);
    }

    /// `syslog_action_restricted` and `check_syslog_permissions`: `CAP_SYSLOG` for anything
    /// but reading everything or the size, or for those too with `dmesg_restrict`; a reader of
    /// `/proc/kmsg` was checked when it opened it.
    fn check_permissions(
        &self,
        action: i32,
        source: Source,
        may_syslog: bool,
    ) -> Result<(), Errno> {
        if source == Source::Proc && action != SYSLOG_ACTION_OPEN {
            return Ok(());
        }
        let restricted = self.dmesg_restrict.load(Ordering::Relaxed)
            || (action != SYSLOG_ACTION_READ_ALL && action != SYSLOG_ACTION_SIZE_BUFFER);
        match restricted && !may_syslog {
            true => Err(Errno::EPERM),
            false => Ok(()),
        }
    }

    /// `syslog_print`'s copy: as much of what follows `syslog_seq` as fits `size`, a record
    /// that does not fit the first time in part; consumed. `None` when there is nothing.
    fn syslog_take(&self, size: usize) -> Option<Vec<u8>> {
        let mut log = self.log.lock_or_recover();
        if log.syslog_seq < log.first_seq() {
            log.syslog_seq = log.first_seq();
            log.syslog_partial = 0;
        }
        if log.syslog_seq >= log.next_seq {
            return None;
        }
        let mut out = Vec::new();
        loop {
            let seq = log.syslog_seq;
            let Some(record) = log.from(seq).next() else {
                break;
            };
            let text = record.print_text(true);
            let rest = &text[log.syslog_partial..];
            if out.len() + rest.len() <= size {
                out.extend_from_slice(rest);
                log.syslog_seq += 1;
                log.syslog_partial = 0;
            } else {
                if out.is_empty() {
                    out.extend_from_slice(&rest[..size]);
                    log.syslog_partial += size;
                }
                break;
            }
        }
        Some(out)
    }

    /// `syslog_print_all`: the newest records from `clear_seq` that fit `size` together,
    /// oldest first; with `clear`, `clear_seq` moves past them.
    fn read_all(&self, size: usize, clear: bool) -> Vec<u8> {
        let mut log = self.log.lock_or_recover();
        let texts: Vec<Vec<u8>> = log
            .from(log.clear_seq)
            .map(|r| r.print_text(true))
            .collect();
        let mut total = 0;
        let fitting = texts
            .iter()
            .rev()
            .take_while(|text| {
                total += text.len();
                total <= size
            })
            .count();
        let out = texts[texts.len() - fitting..].concat();
        if clear {
            log.clear_seq = log.next_seq;
        }
        out
    }

    /// `SYSLOG_ACTION_SIZE_UNREAD`: the bytes `syslog_print` would give; from `/proc/kmsg`
    /// the number of records.
    fn size_unread(&self, source: Source) -> usize {
        let mut log = self.log.lock_or_recover();
        if log.syslog_seq < log.first_seq() {
            log.syslog_seq = log.first_seq();
            log.syslog_partial = 0;
        }
        if source == Source::Proc {
            return (log.next_seq - log.syslog_seq) as usize;
        }
        let total: usize = log
            .from(log.syslog_seq)
            .map(|r| r.print_text(true).len())
            .sum();
        total.saturating_sub(log.syslog_partial)
    }

    /// `kernel.printk`'s four levels.
    pub(crate) fn levels(&self) -> [i32; 4] {
        [
            &self.console_loglevel,
            &self.default_message_loglevel,
            &self.minimum_console_loglevel,
            &self.default_console_loglevel,
        ]
        .map(|level| level.load(Ordering::Relaxed))
    }

    /// Writes `kernel.printk`: as many of the levels as are given.
    pub(crate) fn set_levels(&self, levels: &[i32]) {
        let slots = [
            &self.console_loglevel,
            &self.default_message_loglevel,
            &self.minimum_console_loglevel,
            &self.default_console_loglevel,
        ];
        for (slot, &level) in slots.iter().zip(levels) {
            slot.store(level, Ordering::Relaxed);
        }
    }

    pub(crate) fn dmesg_restrict(&self) -> bool {
        self.dmesg_restrict.load(Ordering::Relaxed)
    }

    pub(crate) fn set_dmesg_restrict(&self, restrict: bool) {
        self.dmesg_restrict.store(restrict, Ordering::Relaxed);
    }

    pub(crate) fn devkmsg(&self) -> Devkmsg {
        match self.devkmsg.load(Ordering::Relaxed) {
            0 => Devkmsg::On,
            1 => Devkmsg::Off,
            _ => Devkmsg::Ratelimit,
        }
    }

    pub(crate) fn set_devkmsg(&self, mode: Devkmsg) {
        self.devkmsg.store(mode as u8, Ordering::Relaxed);
    }

    /// `do_syslog`'s actions that need no copy to the caller.
    fn control(&self, action: i32, len: i32, source: Source) -> Result<i32, Errno> {
        Ok(match action {
            SYSLOG_ACTION_CLOSE | SYSLOG_ACTION_OPEN => 0,
            SYSLOG_ACTION_CLEAR => {
                let mut log = self.log.lock_or_recover();
                log.clear_seq = log.next_seq;
                0
            }
            SYSLOG_ACTION_CONSOLE_OFF => {
                let current = self.console_loglevel.load(Ordering::Relaxed);
                let _ = self.saved_console_loglevel.compare_exchange(
                    LOGLEVEL_DEFAULT,
                    current,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                );
                let minimum = self.minimum_console_loglevel.load(Ordering::Relaxed);
                self.console_loglevel.store(minimum, Ordering::Relaxed);
                0
            }
            SYSLOG_ACTION_CONSOLE_ON => {
                let saved = self
                    .saved_console_loglevel
                    .swap(LOGLEVEL_DEFAULT, Ordering::Relaxed);
                if saved != LOGLEVEL_DEFAULT {
                    self.console_loglevel.store(saved, Ordering::Relaxed);
                }
                0
            }
            SYSLOG_ACTION_CONSOLE_LEVEL => {
                if !(1..=8).contains(&len) {
                    return Err(Errno::EINVAL);
                }
                let minimum = self.minimum_console_loglevel.load(Ordering::Relaxed);
                self.console_loglevel
                    .store(len.max(minimum), Ordering::Relaxed);
                self.saved_console_loglevel
                    .store(LOGLEVEL_DEFAULT, Ordering::Relaxed);
                0
            }
            SYSLOG_ACTION_SIZE_UNREAD => self.size_unread(source) as i32,
            SYSLOG_ACTION_SIZE_BUFFER => LOG_BUF_LEN as i32,
            _ => return Err(Errno::EINVAL),
        })
    }
}

impl Kernel {
    /// Adds `text` to the kernel log at `level` (`printk`), as the kernel's own messages and
    /// the embedder's warnings about the machine are.
    pub fn printk(&self, level: Level, text: &str) {
        self.printk.log(level, text);
    }

    /// Whether the caller may read the kernel log where it is restricted (`CAP_SYSLOG`).
    fn may_syslog(&self, caller: &Caller<'_>) -> bool {
        self.cred(caller).capable_init(Cap::Syslog)
    }

    /// `syslog(2)` (`do_syslog` from `SYSLOG_FROM_READER`).
    pub(crate) async fn sys_syslog(
        &self,
        caller: &Caller<'_>,
        action: i32,
        buf: Buf,
        len: i32,
    ) -> SysResult<i32> {
        self.do_syslog(caller, action, buf.addr(), len, Source::Reader)
            .await
    }

    async fn do_syslog(
        &self,
        caller: &Caller<'_>,
        action: i32,
        buf: u32,
        len: i32,
        source: Source,
    ) -> SysResult<i32> {
        let printk = &self.printk;
        printk.check_permissions(action, source, self.may_syslog(caller))?;
        let data = match action {
            SYSLOG_ACTION_READ | SYSLOG_ACTION_READ_ALL | SYSLOG_ACTION_READ_CLEAR => {
                if buf == 0 || len < 0 {
                    return Err(Errno::EINVAL.into());
                }
                if len == 0 {
                    return Ok(0);
                }
                let size = len as usize;
                match action {
                    SYSLOG_ACTION_READ => {
                        let interrupt = self.interrupt(caller);
                        let take = || printk.syslog_take(size);
                        wait_event(&printk.wait, take, &*interrupt)
                            .await
                            .map_err(|_| SysErr::Restart)?
                    }
                    _ => printk.read_all(size, action == SYSLOG_ACTION_READ_CLEAR),
                }
            }
            _ => return Ok(printk.control(action, len, source)?),
        };
        caller.memory().write(buf, &data).await?;
        Ok(data.len() as i32)
    }

    /// Opens `/proc/kmsg` (`kmsg_open`: `do_syslog(SYSLOG_ACTION_OPEN, SYSLOG_FROM_PROC)`).
    pub(crate) fn open_proc_kmsg(&self, caller: &Caller<'_>) -> Result<Arc<dyn FileObject>, Errno> {
        let printk = &self.printk;
        printk.check_permissions(SYSLOG_ACTION_OPEN, Source::Proc, self.may_syslog(caller))?;
        Ok(Arc::new(ProcKmsg(self.printk.clone())))
    }

    /// Opens `/dev/kmsg` (`devkmsg_open`): reading takes what `dmesg` may read; the reader
    /// starts at the oldest record.
    pub(crate) fn open_dev_kmsg(
        &self,
        caller: &Caller<'_>,
        file: File,
        flags: OpenFlags,
    ) -> Result<Arc<dyn FileObject>, Errno> {
        let printk = &self.printk;
        if printk.devkmsg() == Devkmsg::Off {
            return Err(Errno::EPERM);
        }
        if flags & OpenFlags::O_ACCMODE != OpenFlags::O_WRONLY {
            let may = self.may_syslog(caller);
            printk.check_permissions(SYSLOG_ACTION_READ_ALL, Source::Reader, may)?;
        }
        let seq = printk.log.lock_or_recover().first_seq();
        Ok(Arc::new(DevKmsg {
            printk: printk.clone(),
            _file: file,
            seq: Mutex::new(seq),
            ratelimit: Mutex::default(),
        }))
    }
}

/// An open `/proc/kmsg`: reads consume the log, as `SYSLOG_ACTION_READ` does.
struct ProcKmsg(Arc<Printk>);

impl Pollable for ProcKmsg {
    /// `kmsg_poll`.
    fn poll(&self, mut table: Option<&mut PollTable<'_>>) -> PollMask {
        poll_wait(&mut table, &self.0.wait);
        match self.0.size_unread(Source::Proc) {
            0 => PollMask::empty(),
            _ => PollMask::IN | PollMask::RDNORM,
        }
    }
}

impl FileObject for ProcKmsg {
    /// `kmsg_read`: `EAGAIN` without blocking when nothing is unread.
    fn read<'a>(&'a self, io: Io<'a>, buf: &'a mut [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async move {
            if buf.is_empty() {
                return Ok(0);
            }
            if io.nonblocking() && self.0.size_unread(Source::Proc) == 0 {
                return Err(Errno::EAGAIN.into());
            }
            let take = || self.0.syslog_take(buf.len());
            let data = wait_event(&self.0.wait, take, io.interrupt)
                .await
                .map_err(|_| SysErr::Restart)?;
            buf[..data.len()].copy_from_slice(&data);
            Ok(data.len())
        })
    }

    fn write<'a>(&'a self, _: Io<'a>, _: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async { Err(Errno::EINVAL.into()) })
    }
}

/// An open `/dev/kmsg` (`struct devkmsg_user`).
struct DevKmsg {
    printk: Arc<Printk>,
    _file: File,
    /// The next record it reads.
    seq: Mutex<u64>,
    /// When its burst of writes started and how many it had (`ratelimit_state`).
    ratelimit: Mutex<(Option<Duration>, u32)>,
}

impl DevKmsg {
    /// `___ratelimit` with the default interval and burst: whether this write is logged.
    fn ratelimit(&self) -> bool {
        let now = self.printk.host.monotonic();
        let mut state = self.ratelimit.lock_or_recover();
        let (begin, count) = &mut *state;
        if begin.is_none_or(|begin| now >= begin + RATELIMIT_INTERVAL) {
            *begin = Some(now);
            *count = 0;
        }
        *count += 1;
        *count <= RATELIMIT_BURST
    }

    /// `printk_get_next_message` from the reader's place: the record there or the next one
    /// left, and whether some were lost before it.
    fn next(&self) -> Option<(Vec<u8>, u64, bool)> {
        let seq = *self.seq.lock_or_recover();
        let log = self.printk.log.lock_or_recover();
        let record = log.from(seq).next()?;
        Some((record.ext_text(), record.seq, record.seq != seq))
    }
}

impl Pollable for DevKmsg {
    /// `devkmsg_poll`: readable with a record to read, and an error once records were lost.
    fn poll(&self, mut table: Option<&mut PollTable<'_>>) -> PollMask {
        poll_wait(&mut table, &self.printk.wait);
        match self.next() {
            None => PollMask::empty(),
            Some((_, _, false)) => PollMask::IN | PollMask::RDNORM,
            Some((_, _, true)) => PollMask::IN | PollMask::RDNORM | PollMask::ERR | PollMask::PRI,
        }
    }
}

impl FileObject for DevKmsg {
    /// `devkmsg_read`: one record per read; `EPIPE` once when records were lost before the
    /// reader's place, `EINVAL` for a buffer it does not fit.
    fn read<'a>(&'a self, io: Io<'a>, buf: &'a mut [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async move {
            let next = match self.next() {
                Some(next) => next,
                None if io.nonblocking() => return Err(Errno::EAGAIN.into()),
                None => wait_event(&self.printk.wait, || self.next(), io.interrupt)
                    .await
                    .map_err(|_| SysErr::Restart)?,
            };
            let (text, seq, dropped) = next;
            let mut at = self.seq.lock_or_recover();
            if dropped {
                *at = seq;
                return Err(Errno::EPIPE.into());
            }
            *at = seq + 1;
            if text.len() > buf.len() {
                return Err(Errno::EINVAL.into());
            }
            buf[..text.len()].copy_from_slice(&text);
            Ok(text.len())
        })
    }

    /// `devkmsg_write`: one record of the user facility at the default level, or as a
    /// leading `<prio>` says (a facility of 0 stays the user's); off, or past the rate limit,
    /// the message is dropped.
    fn write<'a>(&'a self, _: Io<'a>, data: &'a [u8]) -> LocalFuture<'a, SysResult<usize>> {
        Box::pin(async move {
            if data.len() > RECORD_MAX {
                return Err(Errno::EINVAL.into());
            }
            match self.printk.devkmsg() {
                Devkmsg::Off => return Ok(data.len()),
                Devkmsg::Ratelimit if !self.ratelimit() => return Ok(data.len()),
                _ => {}
            }
            let mut level = self.printk.default_message_loglevel.load(Ordering::Relaxed) as u8;
            let mut facility = LOG_USER;
            let mut line = data;
            if let Some(rest) = data.strip_prefix(b"<") {
                let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
                if rest.get(digits) == Some(&b'>') {
                    // `simple_strtoul`: no digits is 0.
                    let prio = std::str::from_utf8(&rest[..digits])
                        .ok()
                        .and_then(|d| d.parse::<u32>().ok())
                        .unwrap_or(0);
                    level = (prio & 7) as u8;
                    if prio >> 3 != 0 {
                        facility = (prio >> 3) as u8;
                    }
                    line = &rest[digits + 1..];
                }
            }
            let line = line.split(|&b| b == 0).next().unwrap_or_default();
            self.printk.emit(facility, level, line);
            Ok(data.len())
        })
    }

    fn positional(&self) -> bool {
        true
    }

    /// `devkmsg_llseek`: only to the first record, past what `SYSLOG_ACTION_CLEAR` cleared,
    /// or to the end, with an offset of 0.
    fn llseek(&self, _: u64, base: SeekBase, offset: i64) -> Result<u64, Errno> {
        if offset != 0 {
            return Err(Errno::ESPIPE);
        }
        let log = self.printk.log.lock_or_recover();
        let seq = match base {
            SeekBase::Start => log.first_seq(),
            SeekBase::Data => log.clear_seq,
            SeekBase::End => log.next_seq,
            _ => return Err(Errno::EINVAL),
        };
        *self.seq.lock_or_recover() = seq;
        Ok(0)
    }
}
