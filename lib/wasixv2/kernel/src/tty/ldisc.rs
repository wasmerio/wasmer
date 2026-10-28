//! The line discipline, Linux's `n_tty` (`drivers/tty/n_tty.c`): what a terminal does with the
//! bytes it receives before a reader gets them (canonical line editing, echo, the signal
//! characters, flow control, input translation) and with the bytes written to it before they
//! reach the device (output post-processing).
//!
//! This is the discipline's state alone, without locks, waits or signals: the terminal holds it
//! under one lock (`termios_rwsem` with the `n_tty_data` it guards) and acts on what each
//! operation reports, such as signals to send or echoes to write. Left out: parity and break
//! (no device here has them), `IUCLC`, `OLCUC` and `XCASE`, `ECHOPRT`, output delays, and
//! `EXTPROC`. A pty master's packet mode is the terminal's, around the discipline's read.

use std::collections::VecDeque;

use wasixv2_abi::{
    Signal, Termios, TermiosCcIndex, TermiosCflag, TermiosIflag, TermiosLflag, TermiosOflag,
};

/// The read buffer's size (`N_TTY_BUF_SIZE`): a canonical line holds at most 4095 bytes and its
/// end.
pub(crate) const N_TTY_BUF_SIZE: usize = 4096;

/// A control character that is turned off (`__DISABLED_CHAR`, `_POSIX_VDISABLE`). It also marks
/// the end of a line ended by `VEOF`, which a read does not return.
const DISABLED: u8 = 0;

/// The termios a terminal starts with (`tty_std_termios`): canonical mode with echo and the
/// signal characters, `^C` `^\` DEL `^U` `^D` `^Q` `^S` `^Z` `^R` `^O` `^W` `^V`, 38400 baud.
pub(crate) fn std_termios() -> Termios {
    let mut c_cc = [0; 19];
    // INIT_C_CC.
    for (index, c) in [
        (TermiosCcIndex::VINTR, 0o003),
        (TermiosCcIndex::VQUIT, 0o034),
        (TermiosCcIndex::VERASE, 0o177),
        (TermiosCcIndex::VKILL, 0o025),
        (TermiosCcIndex::VEOF, 0o004),
        (TermiosCcIndex::VMIN, 1),
        (TermiosCcIndex::VSTART, 0o021),
        (TermiosCcIndex::VSTOP, 0o023),
        (TermiosCcIndex::VSUSP, 0o032),
        (TermiosCcIndex::VREPRINT, 0o022),
        (TermiosCcIndex::VDISCARD, 0o017),
        (TermiosCcIndex::VWERASE, 0o027),
        (TermiosCcIndex::VLNEXT, 0o026),
    ] {
        c_cc[index.0 as usize] = c;
    }
    Termios {
        c_iflag: TermiosIflag::ICRNL | TermiosIflag::IXON,
        c_oflag: TermiosOflag::OPOST | TermiosOflag::ONLCR,
        c_cflag: TermiosCflag(TermiosCflag::CS8.0 | TermiosCflag::CREAD.0 | TermiosCflag::HUPCL.0)
            | TermiosCflag(B38400),
        c_lflag: TermiosLflag::ISIG
            | TermiosLflag::ICANON
            | TermiosLflag::ECHO
            | TermiosLflag::ECHOE
            | TermiosLflag::ECHOK
            | TermiosLflag::ECHOCTL
            | TermiosLflag::ECHOKE
            | TermiosLflag::IEXTEN,
        c_line: 0,
        c_cc,
    }
}

/// `B38400` in `c_cflag`'s `CBAUD` field.
pub(crate) const B38400: u32 = 0o17;

/// The baud rate `c_cflag`'s `CBAUD` field names (`tty_termios_baud_rate`, `baud_table`).
pub(crate) fn baud_rate(cflag: TermiosCflag) -> u32 {
    const TABLE: [u32; 16] = [
        0, 50, 75, 110, 134, 150, 200, 300, 600, 1200, 1800, 2400, 4800, 9600, 19200, 38400,
    ];
    const EXTENDED: [u32; 15] = [
        57600, 115200, 230400, 460800, 500000, 576000, 921600, 1000000, 1152000, 1500000, 2000000,
        2500000, 3000000, 3500000, 4000000,
    ];
    let cbaud = cflag.0 & TermiosCflag::CBAUD.0;
    match cbaud & TermiosCflag::CBAUDEX.0 {
        0 => TABLE[cbaud as usize],
        _ => EXTENDED
            .get((cbaud & !TermiosCflag::CBAUDEX.0) as usize - 1)
            .copied()
            // BOTHER: the speed is in `c_ospeed`, which `struct termios` does not carry.
            .unwrap_or(0),
    }
}

/// One byte of the read buffer and whether it ends a canonical line (`read_flags`).
#[derive(Clone, Copy)]
struct Slot {
    c: u8,
    eol: bool,
}

/// What echo writes, kept until the echoes are processed against the output column (Linux's
/// echo buffer with its `ECHO_OP_*` operations).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Echo {
    /// A byte, post-processed as output.
    Byte(u8),
    /// A control character as `^X`.
    Ctrl(u8),
    /// Back over a tab: `chars` columns (modulo 8) were used since the start of input, or since
    /// a previous tab if `after_tab`.
    EraseTab { chars: u8, after_tab: bool },
    /// The canonical line starts at the current column.
    SetCanonCol,
}

/// What [`Ldisc::receive`] did besides taking the input.
#[derive(Default, Debug, PartialEq, Eq)]
pub(crate) struct Received {
    /// How many bytes it took; the rest wait for room in the read buffer.
    pub(crate) taken: usize,
    /// Signals for the foreground process group (`isig`), in order.
    pub(crate) signals: Vec<i32>,
    /// Input is ready for a reader.
    pub(crate) readable: bool,
    /// Output started again (`^Q`), so blocked writers may go on.
    pub(crate) started: bool,
    /// Output stopped (`^S`).
    pub(crate) stopped: bool,
    /// A signal character flushed the unread input (`isig` without `NOFLSH`).
    pub(crate) flushed: bool,
}

/// The discipline's state and the termios it follows.
pub(crate) struct Ldisc {
    pub(crate) termios: Termios,
    /// Received bytes not read yet; the front is `read_tail`, the back `read_head`.
    buf: VecDeque<Slot>,
    /// How many bytes at the front belong to finished lines (`canon_head - read_tail`); only in
    /// canonical mode.
    canon: usize,
    /// The next byte is taken literally (`VLNEXT`).
    lnext: bool,
    /// The output column, and the column the canonical line started at.
    column: usize,
    canon_column: usize,
    echo: VecDeque<Echo>,
    /// Output is stopped: by `VSTOP` (`flow.stopped`), or by `TCOOFF` too (`tco_stopped`).
    stopped: bool,
    tco_stopped: bool,
}

/// `iscntrl` in the C locale.
fn is_cntrl(c: u8) -> bool {
    c < 0x20 || c == 0x7f
}

impl Ldisc {
    pub(crate) fn new(termios: Termios) -> Self {
        Self {
            termios,
            buf: VecDeque::new(),
            canon: 0,
            lnext: false,
            column: 0,
            canon_column: 0,
            echo: VecDeque::new(),
            stopped: false,
            tco_stopped: false,
        }
    }

    fn iflag(&self, flag: TermiosIflag) -> bool {
        self.termios.c_iflag.0 & flag.0 != 0
    }

    fn oflag(&self, flag: TermiosOflag) -> bool {
        self.termios.c_oflag.0 & flag.0 != 0
    }

    fn lflag(&self, flag: TermiosLflag) -> bool {
        self.termios.c_lflag.0 & flag.0 != 0
    }

    fn cc(&self, index: TermiosCcIndex) -> u8 {
        self.termios.c_cc[index.0 as usize]
    }

    pub(crate) fn icanon(&self) -> bool {
        self.lflag(TermiosLflag::ICANON)
    }

    pub(crate) fn vmin(&self) -> u8 {
        self.cc(TermiosCcIndex::VMIN)
    }

    pub(crate) fn vtime(&self) -> u8 {
        self.cc(TermiosCcIndex::VTIME)
    }

    pub(crate) fn tostop(&self) -> bool {
        self.lflag(TermiosLflag::TOSTOP)
    }

    pub(crate) fn stopped(&self) -> bool {
        self.stopped
    }

    /// Whether `c` continues a UTF-8 character, which takes no column of its own and is erased
    /// with the byte that starts it (`is_continuation` under `IUTF8`).
    fn is_continuation(&self, c: u8) -> bool {
        self.iflag(TermiosIflag::IUTF8) && c & 0xc0 == 0x80
    }

    /// `n_tty_set_termios`: installs `termios`. Switching canonical mode on or off rebuilds
    /// the line state: what was received before switching to canonical mode is one line.
    pub(crate) fn set_termios(&mut self, termios: Termios) -> Received {
        let old = std::mem::replace(&mut self.termios, termios);
        let modes = TermiosLflag::ICANON.0 | TermiosLflag::EXTPROC.0;
        if (old.c_lflag.0 ^ termios.c_lflag.0) & modes != 0 {
            for slot in &mut self.buf {
                slot.eol = false;
            }
            // What was received before switching to canonical mode is one line, without a
            // delimiter.
            if !self.icanon() || self.buf.is_empty() {
                self.canon = 0;
            } else {
                self.buf.back_mut().expect("not empty").eol = true;
                self.canon = self.buf.len();
            }
            self.lnext = false;
        }
        let mut received = Received::default();
        // Turning IXON off must not leave output stopped by a `VSTOP` that came before.
        if !self.iflag(TermiosIflag::IXON)
            && old.c_iflag.0 & TermiosIflag::IXON.0 != 0
            && !self.tco_stopped
        {
            received.started = self.start();
        }
        received.readable = true;
        received
    }

    // ---- input ----

    /// `n_tty_receive_buf_common`: takes as much of `input` as there is room for, and says what
    /// that did. In canonical mode a line that does not fit is cut at 4095 bytes; the bytes
    /// after it are still processed, so erasing and the line's end always get through.
    pub(crate) fn receive(&mut self, input: &[u8]) -> Received {
        let mut received = Received::default();
        for &c in input {
            let parmrk = self.iflag(TermiosIflag::PARMRK);
            let free = N_TTY_BUF_SIZE as isize - self.buf.len() as isize;
            let room = match parmrk {
                true => (free + 2) / 3,
                false => free,
            } - 1;
            if room <= 0 {
                let overflow = self.icanon() && self.canon == 0;
                if !overflow {
                    break;
                }
                if room < 0 {
                    self.buf.pop_back();
                }
            }
            self.receive_char_any(c, &mut received);
            received.taken += 1;
        }
        // Without canonical mode, everything received is for readers at once (`commit_head`).
        if !self.icanon() && !self.buf.is_empty() {
            received.readable = true;
        }
        received
    }

    /// `n_tty_receive_buf_standard`, or the raw path when no input processing is on.
    fn receive_char_any(&mut self, mut c: u8, received: &mut Received) {
        if self.is_raw() {
            self.put(c);
            return;
        }
        if self.lnext {
            self.lnext = false;
            if self.iflag(TermiosIflag::ISTRIP) {
                c &= 0x7f;
            }
            self.receive_char(c, received);
            return;
        }
        if self.iflag(TermiosIflag::ISTRIP) {
            c &= 0x7f;
        }
        if self.lflag(TermiosLflag::EXTPROC) {
            self.put(c);
            return;
        }
        match self.is_special(c) {
            true => self.receive_char_special(c, received),
            false => self.receive_char(c, received),
        }
    }

    /// No byte needs processing (`ldata->raw`): `n_tty_set_termios` with none of the input
    /// processing flags.
    fn is_raw(&self) -> bool {
        let iflags = TermiosIflag::ISTRIP.0
            | TermiosIflag::IGNCR.0
            | TermiosIflag::ICRNL.0
            | TermiosIflag::INLCR.0
            | TermiosIflag::IXON.0
            | TermiosIflag::PARMRK.0;
        let lflags = TermiosLflag::ICANON.0 | TermiosLflag::ISIG.0 | TermiosLflag::ECHO.0;
        self.termios.c_iflag.0 & iflags == 0 && self.termios.c_lflag.0 & lflags == 0
    }

    /// Whether `c` is in `char_map`, the bytes `n_tty_receive_char_special` handles.
    fn is_special(&self, c: u8) -> bool {
        use TermiosCcIndex as V;
        if c == DISABLED {
            return false;
        }
        let is = |index| self.cc(index) == c;
        (c == b'\r' && (self.iflag(TermiosIflag::IGNCR) || self.iflag(TermiosIflag::ICRNL)))
            || (c == b'\n' && self.iflag(TermiosIflag::INLCR))
            || (self.icanon()
                && (is(V::VERASE) || is(V::VKILL) || is(V::VEOF) || c == b'\n' || is(V::VEOL)))
            || (self.icanon()
                && self.lflag(TermiosLflag::IEXTEN)
                && (is(V::VWERASE)
                    || is(V::VLNEXT)
                    || is(V::VEOL2)
                    || (is(V::VREPRINT) && self.lflag(TermiosLflag::ECHO))))
            || (self.iflag(TermiosIflag::IXON) && (is(V::VSTART) || is(V::VSTOP)))
            || (self.lflag(TermiosLflag::ISIG) && (is(V::VINTR) || is(V::VQUIT) || is(V::VSUSP)))
    }

    fn receive_char_special(&mut self, mut c: u8, received: &mut Received) {
        use TermiosCcIndex as V;
        if self.iflag(TermiosIflag::IXON) && (c == self.cc(V::VSTART) || c == self.cc(V::VSTOP)) {
            match c == self.cc(V::VSTART) {
                true => received.started |= self.start(),
                false => received.stopped |= self.stop(),
            }
            return;
        }
        if self.lflag(TermiosLflag::ISIG) {
            let sig = match c {
                c if c == self.cc(V::VINTR) => Some(Signal::SIGINT),
                c if c == self.cc(V::VQUIT) => Some(Signal::SIGQUIT),
                c if c == self.cc(V::VSUSP) => Some(Signal::SIGTSTP),
                _ => None,
            };
            if let Some(sig) = sig {
                self.receive_signal_char(sig.0, c, received);
                return;
            }
        }
        self.restart_on_any(received);
        if c == b'\r' {
            if self.iflag(TermiosIflag::IGNCR) {
                return;
            }
            if self.iflag(TermiosIflag::ICRNL) {
                c = b'\n';
            }
        } else if c == b'\n' && self.iflag(TermiosIflag::INLCR) {
            c = b'\r';
        }
        if self.icanon() && self.receive_char_canon(c, received) {
            return;
        }
        if self.lflag(TermiosLflag::ECHO) {
            if c == b'\n' {
                self.echo.push_back(Echo::Byte(b'\n'));
            } else {
                self.echo_set_canon_col_at_line_start();
                self.echo_char(c);
            }
        }
        self.put_parmrk(c);
    }

    /// `n_tty_receive_char`: an ordinary byte.
    fn receive_char(&mut self, c: u8, received: &mut Received) {
        self.restart_on_any(received);
        if self.lflag(TermiosLflag::ECHO) {
            self.echo_set_canon_col_at_line_start();
            self.echo_char(c);
        }
        self.put_parmrk(c);
    }

    /// `IXANY`: any byte restarts output stopped by `VSTOP`.
    fn restart_on_any(&mut self, received: &mut Received) {
        if self.stopped
            && !self.tco_stopped
            && self.iflag(TermiosIflag::IXON)
            && self.iflag(TermiosIflag::IXANY)
        {
            received.started |= self.start();
        }
    }

    /// `n_tty_receive_signal_char`: `isig`, then the character's echo (`^C`).
    fn receive_signal_char(&mut self, sig: i32, c: u8, received: &mut Received) {
        received.signals.push(sig);
        if !self.lflag(TermiosLflag::NOFLSH) {
            // `isig`'s flush: pending echoes and unread input go.
            self.echo.clear();
            self.flush_input();
            received.flushed = true;
        }
        if self.iflag(TermiosIflag::IXON) {
            received.started |= self.start();
        }
        if self.lflag(TermiosLflag::ECHO) {
            self.echo_char(c);
        }
    }

    /// `n_tty_receive_char_canon`: canonical mode's special bytes; whether `c` was one.
    fn receive_char_canon(&mut self, c: u8, received: &mut Received) -> bool {
        use TermiosCcIndex as V;
        let iexten = self.lflag(TermiosLflag::IEXTEN);
        let echo = self.lflag(TermiosLflag::ECHO);
        if c == self.cc(V::VERASE) || c == self.cc(V::VKILL) || (c == self.cc(V::VWERASE) && iexten)
        {
            self.eraser(c);
            return true;
        }
        if c == self.cc(V::VLNEXT) && iexten {
            self.lnext = true;
            if echo && self.lflag(TermiosLflag::ECHOCTL) {
                self.echo.push_back(Echo::Byte(b'^'));
                self.echo.push_back(Echo::Byte(b'\x08'));
            }
            return true;
        }
        if c == self.cc(V::VREPRINT) && echo && iexten {
            self.echo_char(c);
            self.echo.push_back(Echo::Byte(b'\n'));
            let line: Vec<u8> = self.buf.iter().skip(self.canon).map(|s| s.c).collect();
            for c in line {
                self.echo_char(c);
            }
            return true;
        }
        if c == b'\n' {
            if echo || self.lflag(TermiosLflag::ECHONL) {
                self.echo.push_back(Echo::Byte(b'\n'));
            }
            self.end_line(c, received);
            return true;
        }
        if c == self.cc(V::VEOF) {
            self.end_line(DISABLED, received);
            return true;
        }
        if c == self.cc(V::VEOL) || (c == self.cc(V::VEOL2) && iexten) {
            if echo {
                self.echo_set_canon_col_at_line_start();
                self.echo_char(c);
            }
            if c == 0xff && self.iflag(TermiosIflag::PARMRK) {
                self.put(c);
            }
            self.end_line(c, received);
            return true;
        }
        false
    }

    /// `n_tty_receive_handle_newline`: `c` ends the line, which readers can have now.
    fn end_line(&mut self, c: u8, received: &mut Received) {
        self.put(c);
        if let Some(slot) = self.buf.back_mut() {
            slot.eol = true;
        }
        self.canon = self.buf.len();
        received.readable = true;
    }

    /// `put_tty_queue`; a full buffer drops the byte (a canonical line past its limit).
    fn put(&mut self, c: u8) {
        if self.buf.len() < N_TTY_BUF_SIZE {
            self.buf.push_back(Slot { c, eol: false });
        }
    }

    /// `put_tty_queue` with `PARMRK`'s doubling of 0377.
    fn put_parmrk(&mut self, c: u8) {
        if c == 0xff && self.iflag(TermiosIflag::PARMRK) {
            self.put(c);
        }
        self.put(c);
    }

    /// `eraser`: `VERASE` takes back a character, `VWERASE` a word, `VKILL` the line, and echo
    /// shows it.
    fn eraser(&mut self, c: u8) {
        use TermiosCcIndex as V;
        #[derive(PartialEq)]
        enum Erase {
            Char,
            Word,
            Line,
        }
        if self.buf.len() == self.canon {
            return;
        }
        let echo = self.lflag(TermiosLflag::ECHO);
        let erase = match c {
            c if c == self.cc(V::VERASE) => Erase::Char,
            c if c == self.cc(V::VWERASE) => Erase::Word,
            _ => {
                if !echo {
                    self.buf.truncate(self.canon);
                    return;
                }
                let full = self.lflag(TermiosLflag::ECHOK)
                    && self.lflag(TermiosLflag::ECHOKE)
                    && self.lflag(TermiosLflag::ECHOE);
                if !full {
                    self.buf.truncate(self.canon);
                    self.echo_char(self.cc(V::VKILL));
                    if self.lflag(TermiosLflag::ECHOK) {
                        self.echo.push_back(Echo::Byte(b'\n'));
                    }
                    return;
                }
                Erase::Line
            }
        };
        let echoctl = self.lflag(TermiosLflag::ECHOCTL);
        let mut seen_alnums = 0;
        while self.buf.len() > self.canon {
            // A whole, possibly multibyte, character.
            let mut head = self.buf.len();
            let mut c;
            loop {
                head -= 1;
                c = self.buf[head].c;
                if !self.is_continuation(c) || head == self.canon {
                    break;
                }
            }
            if self.is_continuation(c) {
                break;
            }
            if erase == Erase::Word {
                // BSD's ALTWERASE.
                if c.is_ascii_alphanumeric() || c == b'_' {
                    seen_alnums += 1;
                } else if seen_alnums > 0 {
                    break;
                }
            }
            self.buf.truncate(head);
            if echo {
                if erase == Erase::Char && !self.lflag(TermiosLflag::ECHOE) {
                    self.echo_char(self.cc(V::VERASE));
                } else if c == b'\t' {
                    self.echo_erase_tab();
                } else {
                    let rubout = [Echo::Byte(b'\x08'), Echo::Byte(b' '), Echo::Byte(b'\x08')];
                    if is_cntrl(c) && echoctl {
                        self.echo.extend(rubout);
                    }
                    if !is_cntrl(c) || echoctl {
                        self.echo.extend(rubout);
                    }
                }
            }
            if erase == Erase::Char {
                break;
            }
        }
    }

    /// The columns before an erased tab, counted back to the start of the line or the tab
    /// before it (`eraser`'s tab case and `echo_erase_tab`).
    fn echo_erase_tab(&mut self) {
        let echoctl = self.lflag(TermiosLflag::ECHOCTL);
        let mut chars = 0u32;
        let mut after_tab = false;
        for slot in self.buf.iter().skip(self.canon).rev() {
            match slot.c {
                b'\t' => {
                    after_tab = true;
                    break;
                }
                c if is_cntrl(c) => {
                    if echoctl {
                        chars += 2;
                    }
                }
                c if !self.is_continuation(c) => chars += 1,
                _ => {}
            }
        }
        self.echo.push_back(Echo::EraseTab {
            chars: (chars & 7) as u8,
            after_tab,
        });
    }

    /// `echo_char`: control characters other than tab as `^X` under `ECHOCTL`.
    fn echo_char(&mut self, c: u8) {
        match self.lflag(TermiosLflag::ECHOCTL) && is_cntrl(c) && c != b'\t' {
            true => self.echo.push_back(Echo::Ctrl(c)),
            false => self.echo.push_back(Echo::Byte(c)),
        }
    }

    /// Records the canonical line's start column before its first byte is echoed.
    fn echo_set_canon_col_at_line_start(&mut self) {
        if self.buf.len() == self.canon {
            self.echo.push_back(Echo::SetCanonCol);
        }
    }

    /// `reset_buffer_flags`: unread input is gone.
    pub(crate) fn flush_input(&mut self) {
        self.buf.clear();
        self.canon = 0;
    }

    // ---- flow control ----

    /// `__start_tty`: whether output was stopped by `VSTOP` and goes on now; `TCOOFF` keeps it
    /// stopped.
    pub(crate) fn start(&mut self) -> bool {
        if !self.stopped || self.tco_stopped {
            return false;
        }
        self.stopped = false;
        true
    }

    /// `__stop_tty`: whether output was going and stops now.
    pub(crate) fn stop(&mut self) -> bool {
        !std::mem::replace(&mut self.stopped, true)
    }

    /// `TCXONC`'s `TCOOFF` and `TCOON`; whether output goes on now.
    pub(crate) fn tco_stop(&mut self, stop: bool) -> bool {
        match (stop, self.tco_stopped) {
            (true, false) => {
                self.tco_stopped = true;
                self.stop();
                false
            }
            (false, true) => {
                self.tco_stopped = false;
                self.start()
            }
            _ => false,
        }
    }

    // ---- reading ----

    /// `input_available_p`: in canonical mode a finished line, otherwise a byte, or `VMIN`
    /// bytes for `poll` when `VTIME` is 0.
    pub(crate) fn input_available(&self, poll: bool) -> bool {
        if self.icanon() && !self.lflag(TermiosLflag::EXTPROC) {
            return self.canon > 0;
        }
        let amount = match poll && self.vtime() == 0 && self.vmin() > 0 {
            true => self.vmin() as usize,
            false => 1,
        };
        self.buf.len() >= amount
    }

    /// Moves input to `out` as one read takes it: in canonical mode at most one line, up to
    /// and including its delimiter but not a `VEOF` (`canon_copy_from_read_buf`), otherwise
    /// what there is (`copy_from_read_buf`). Returns how many bytes it wrote.
    pub(crate) fn read(&mut self, out: &mut [u8]) -> usize {
        if !self.icanon() || self.lflag(TermiosLflag::EXTPROC) {
            let n = out.len().min(self.buf.len());
            for (dst, slot) in out.iter_mut().zip(self.buf.drain(..n)) {
                *dst = slot.c;
            }
            return n;
        }
        let n = out.len().min(self.canon);
        let eol = self.buf.iter().take(n).position(|slot| slot.eol);
        let (take, copy) = match eol {
            // The line's end: a VEOF marker is consumed but not returned.
            Some(at) if self.buf[at].c == DISABLED => (at + 1, at),
            Some(at) => (at + 1, at + 1),
            None => (n, n),
        };
        for (dst, slot) in out.iter_mut().zip(self.buf.iter().take(copy)) {
            *dst = slot.c;
        }
        self.buf.drain(..take);
        self.canon -= take;
        if eol.is_none() && copy == out.len() {
            // `canon_skip_eof`: a buffer filled right up to a VEOF eats it.
            if self
                .buf
                .front()
                .is_some_and(|slot| slot.eol && slot.c == DISABLED)
            {
                self.buf.pop_front();
                self.canon -= 1;
            }
        }
        copy
    }

    /// What `TIOCINQ` reports: the bytes of finished lines without their `VEOF` markers in
    /// canonical mode (`inq_canon`), every byte otherwise.
    pub(crate) fn inq(&self) -> usize {
        if !self.icanon() || self.lflag(TermiosLflag::EXTPROC) {
            return self.buf.len();
        }
        self.buf
            .iter()
            .take(self.canon)
            .filter(|slot| !(slot.eol && slot.c == DISABLED))
            .count()
    }

    // ---- output ----

    /// `__process_echoes`: the pending echoes as the bytes the device gets, against the
    /// output column.
    pub(crate) fn take_echoes(&mut self, out: &mut Vec<u8>) {
        while let Some(echo) = self.echo.pop_front() {
            match echo {
                Echo::Byte(c) => match self.oflag(TermiosOflag::OPOST) {
                    true => self.output_char(c, out),
                    false => out.push(c),
                },
                Echo::Ctrl(c) => {
                    out.extend([b'^', c ^ 0o100]);
                    self.column += 2;
                }
                Echo::EraseTab { chars, after_tab } => {
                    let mut chars = chars as usize;
                    if !after_tab {
                        chars += self.canon_column;
                    }
                    for _ in 0..8 - (chars & 7) {
                        out.push(b'\x08');
                        self.column = self.column.saturating_sub(1);
                    }
                }
                Echo::SetCanonCol => self.canon_column = self.column,
            }
        }
    }

    /// Whether echoes wait to be written.
    pub(crate) fn has_echoes(&self) -> bool {
        !self.echo.is_empty()
    }

    /// `n_tty_write`'s output processing: `data` as the device gets it.
    pub(crate) fn output(&mut self, data: &[u8], out: &mut Vec<u8>) {
        if !self.oflag(TermiosOflag::OPOST) {
            out.extend_from_slice(data);
            return;
        }
        for &c in data {
            self.output_char(c, out);
        }
    }

    /// `do_output_char`: one byte with `OPOST` processing, keeping the column.
    fn output_char(&mut self, c: u8, out: &mut Vec<u8>) {
        match c {
            b'\n' => {
                if self.oflag(TermiosOflag::ONLRET) {
                    self.column = 0;
                }
                if self.oflag(TermiosOflag::ONLCR) {
                    self.canon_column = 0;
                    self.column = 0;
                    out.extend(b"\r\n");
                    return;
                }
                self.canon_column = self.column;
            }
            b'\r' => {
                if self.oflag(TermiosOflag::ONOCR) && self.column == 0 {
                    return;
                }
                if self.oflag(TermiosOflag::OCRNL) {
                    if self.oflag(TermiosOflag::ONLRET) {
                        self.canon_column = 0;
                        self.column = 0;
                    }
                    out.push(b'\n');
                    return;
                }
                self.canon_column = 0;
                self.column = 0;
            }
            b'\t' => {
                let spaces = 8 - (self.column & 7);
                self.column += spaces;
                if self.termios.c_oflag.0 & TermiosOflag::TABDLY.0 == TermiosOflag::XTABS.0 {
                    out.extend(std::iter::repeat_n(b' ', spaces));
                    return;
                }
            }
            b'\x08' => self.column = self.column.saturating_sub(1),
            c if !is_cntrl(c) && !self.is_continuation(c) => self.column += 1,
            _ => {}
        }
        out.push(c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ldisc() -> Ldisc {
        Ldisc::new(std_termios())
    }

    fn echoes(ldisc: &mut Ldisc) -> Vec<u8> {
        let mut out = Vec::new();
        ldisc.take_echoes(&mut out);
        out
    }

    fn read_all(ldisc: &mut Ldisc) -> Vec<u8> {
        let mut buf = [0; 8192];
        let n = ldisc.read(&mut buf);
        buf[..n].to_vec()
    }

    #[test]
    fn canonical_lines_are_edited_and_echoed() {
        let mut ld = ldisc();
        let received = ld.receive(b"helo\x7fl\x7flo wrld\x17world\r");
        assert_eq!(received.taken, 21);
        assert!(received.readable);
        assert_eq!(read_all(&mut ld), b"hello world\n");
        // DEL erases with a rubout; ^W the word; CR echoes as CR LF (ICRNL, then ONLCR).
        assert_eq!(
            echoes(&mut ld),
            b"helo\x08 \x08l\x08 \x08lo wrld\x08 \x08\x08 \x08\x08 \x08\x08 \x08world\r\n"
        );
    }

    #[test]
    fn a_line_is_ready_only_once_ended() {
        let mut ld = ldisc();
        assert!(!ld.receive(b"abc").readable);
        assert!(!ld.input_available(false));
        assert!(ld.receive(b"\n").readable);
        assert!(ld.input_available(false));
    }

    #[test]
    fn eof_ends_a_line_without_itself_and_alone_reads_nothing() {
        let mut ld = ldisc();
        ld.receive(b"ab\x04\x04");
        let mut buf = [0; 16];
        assert_eq!(ld.read(&mut buf), 2);
        assert_eq!(&buf[..2], b"ab");
        assert!(ld.input_available(false));
        assert_eq!(ld.read(&mut buf), 0);
        assert!(!ld.input_available(false));
    }

    #[test]
    fn one_read_takes_one_line() {
        let mut ld = ldisc();
        ld.receive(b"one\ntwo\n");
        assert_eq!(ld.inq(), 8);
        assert_eq!(read_all(&mut ld), b"one\n");
        assert_eq!(read_all(&mut ld), b"two\n");
    }

    #[test]
    fn a_short_buffer_takes_part_of_the_line_and_eats_an_eof_it_fills_up_to() {
        let mut ld = ldisc();
        ld.receive(b"abcd\x04");
        let mut buf = [0; 4];
        assert_eq!(ld.read(&mut buf), 4);
        assert!(!ld.input_available(false));
    }

    #[test]
    fn kill_erases_the_line() {
        let mut ld = ldisc();
        ld.receive(b"abc\x15d\n");
        assert_eq!(read_all(&mut ld), b"d\n");
        // ECHOKE: each character is rubbed out.
        let rubout = b"\x08 \x08";
        let mut expected = b"abc".to_vec();
        for _ in 0..3 {
            expected.extend(rubout);
        }
        expected.extend(b"d\r\n");
        assert_eq!(echoes(&mut ld), expected);
    }

    #[test]
    fn signal_characters_flush_and_echo() {
        let mut ld = ldisc();
        let received = ld.receive(b"abc\x03x\x1c\x1a");
        assert_eq!(
            received.signals,
            [Signal::SIGINT.0, Signal::SIGQUIT.0, Signal::SIGTSTP.0]
        );
        assert_eq!(ld.inq(), 0);
        ld.receive(b"\n");
        assert_eq!(read_all(&mut ld), b"\n");
        // Each flush drops the echoes still pending: only the last signal's is left.
        assert_eq!(echoes(&mut ld), b"^Z\r\n");
    }

    #[test]
    fn noflsh_keeps_the_input() {
        let mut ld = ldisc();
        ld.termios.c_lflag.0 |= TermiosLflag::NOFLSH.0;
        ld.receive(b"ab\x03c\n");
        assert_eq!(read_all(&mut ld), b"abc\n");
    }

    #[test]
    fn lnext_quotes_a_special_character() {
        let mut ld = ldisc();
        ld.receive(b"a\x16\x03\n");
        assert_eq!(read_all(&mut ld), b"a\x03\n");
        assert_eq!(echoes(&mut ld), b"a^\x08^C\r\n");
    }

    #[test]
    fn raw_mode_passes_everything_and_honours_vmin() {
        let mut ld = ldisc();
        let mut raw = ld.termios;
        raw.c_iflag = TermiosIflag(0);
        raw.c_lflag = TermiosLflag(0);
        raw.c_cc[TermiosCcIndex::VMIN.0 as usize] = 3;
        ld.set_termios(raw);
        let received = ld.receive(b"\x03\r");
        assert!(received.signals.is_empty());
        assert!(received.readable);
        assert!(ld.input_available(false));
        assert!(!ld.input_available(true));
        ld.receive(b"x");
        assert!(ld.input_available(true));
        assert_eq!(read_all(&mut ld), b"\x03\rx");
        assert!(echoes(&mut ld).is_empty());
    }

    #[test]
    fn switching_to_canonical_mode_pushes_what_came_before() {
        let mut ld = ldisc();
        let mut raw = ld.termios;
        raw.c_lflag.0 &= !TermiosLflag::ICANON.0;
        ld.set_termios(raw);
        ld.receive(b"ab");
        ld.set_termios(std_termios());
        assert!(ld.input_available(false));
        assert_eq!(read_all(&mut ld), b"ab");
    }

    #[test]
    fn a_canonical_line_is_cut_at_its_limit() {
        let mut ld = ldisc();
        let long = vec![b'a'; 5000];
        assert_eq!(ld.receive(&long).taken, 5000);
        ld.receive(b"\n");
        let line = read_all(&mut ld);
        assert_eq!(line.len(), N_TTY_BUF_SIZE);
        assert_eq!(line.last(), Some(&b'\n'));
    }

    #[test]
    fn raw_input_waits_for_room() {
        let mut ld = ldisc();
        let mut raw = ld.termios;
        raw.c_lflag.0 &= !TermiosLflag::ICANON.0;
        ld.set_termios(raw);
        assert_eq!(ld.receive(&[b'a'; 5000]).taken, N_TTY_BUF_SIZE - 1);
        assert_eq!(ld.receive(b"b").taken, 0);
        let mut buf = [0; 10];
        ld.read(&mut buf);
        assert_eq!(ld.receive(b"b").taken, 1);
    }

    #[test]
    fn ixon_stops_and_starts_output() {
        let mut ld = ldisc();
        let received = ld.receive(b"\x13");
        assert!(ld.stopped());
        assert!(!received.started);
        assert!(ld.receive(b"\x11").started);
        assert!(!ld.stopped());
        assert_eq!(ld.inq(), 0);
        // TCOOFF holds output against VSTART.
        ld.tco_stop(true);
        assert!(!ld.receive(b"\x11").started);
        assert!(ld.tco_stop(false));
    }

    #[test]
    fn output_maps_newlines_and_tracks_the_column_for_tab_erasure() {
        let mut ld = ldisc();
        let mut out = Vec::new();
        ld.output(b"ab\ncd", &mut out);
        assert_eq!(out, b"ab\r\ncd");
        // A prompt of 2 columns, then a tab echoed and erased: back to column 2.
        ld.receive(b"\t\x7f");
        out.clear();
        ld.take_echoes(&mut out);
        assert_eq!(out, b"\t\x08\x08\x08\x08\x08\x08");
    }

    #[test]
    fn utf8_characters_erase_whole() {
        let mut ld = ldisc();
        ld.termios.c_iflag.0 |= TermiosIflag::IUTF8.0;
        ld.receive("aé\x7f\n".as_bytes());
        assert_eq!(read_all(&mut ld), b"a\n");
    }

    #[test]
    fn baud_rates() {
        assert_eq!(baud_rate(std_termios().c_cflag), 38400);
        assert_eq!(baud_rate(TermiosCflag(TermiosCflag::CBAUDEX.0 | 2)), 115200);
    }
}
