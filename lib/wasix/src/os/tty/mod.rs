use std::sync::{Arc, Mutex};

use futures::future::BoxFuture;
use virtual_fs::{AsyncWriteExt, NullFile, VirtualFile};
use wasmer_wasix_types::wasi::{Signal, Snapshot0Clockid};

use crate::syscalls::platform_clock_time_get;

use super::task::signal::SignalHandlerAbi;

const TTY_MOBILE_PAUSE: u128 = std::time::Duration::from_millis(200).as_nanos();

pub mod tty_sys;

#[derive(Debug)]
pub enum InputEvent {
    Key,
    Data(String),
    Raw(Vec<u8>),
}

#[derive(Clone, Debug)]
pub struct ConsoleRect {
    pub cols: u32,
    pub rows: u32,
}

impl Default for ConsoleRect {
    fn default() -> Self {
        Self { cols: 80, rows: 25 }
    }
}

#[derive(Clone, Debug)]
pub struct TtyOptionsInner {
    echo: bool,
    line_buffering: bool,
    // Kept for TTY bridge/syscall/journal compatibility; currently unused by this parser path.
    line_feeds: bool,
    ignore_cr: bool,
    map_cr_to_lf: bool,
    map_lf_to_cr: bool,
    rect: ConsoleRect,
}

#[derive(Debug, Clone)]
pub struct TtyOptions {
    inner: Arc<Mutex<TtyOptionsInner>>,
}

impl Default for TtyOptions {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(TtyOptionsInner {
                echo: true,
                line_buffering: true,
                line_feeds: true,
                ignore_cr: false,
                map_cr_to_lf: true,
                map_lf_to_cr: false,
                rect: ConsoleRect { cols: 80, rows: 25 },
            })),
        }
    }
}

impl TtyOptions {
    pub fn cols(&self) -> u32 {
        let inner = self.inner.lock().unwrap();
        inner.rect.cols
    }

    pub fn set_cols(&self, cols: u32) {
        let mut inner = self.inner.lock().unwrap();
        inner.rect.cols = cols;
    }

    pub fn rows(&self) -> u32 {
        let inner = self.inner.lock().unwrap();
        inner.rect.rows
    }

    pub fn set_rows(&self, rows: u32) {
        let mut inner = self.inner.lock().unwrap();
        inner.rect.rows = rows;
    }

    pub fn echo(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.echo
    }

    pub fn set_echo(&self, echo: bool) {
        let mut inner = self.inner.lock().unwrap();
        inner.echo = echo;
    }

    pub fn line_buffering(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.line_buffering
    }

    pub fn set_line_buffering(&self, line_buffering: bool) {
        let mut inner = self.inner.lock().unwrap();
        inner.line_buffering = line_buffering;
    }

    pub fn line_feeds(&self) -> bool {
        let inner = self.inner.lock().unwrap();
        inner.line_feeds
    }

    pub fn set_line_feeds(&self, line_feeds: bool) {
        let mut inner = self.inner.lock().unwrap();
        inner.line_feeds = line_feeds;
    }

    pub fn ignore_cr(&self) -> bool {
        self.inner.lock().unwrap().ignore_cr
    }

    pub fn set_ignore_cr(&self, ignore_cr: bool) {
        let mut inner = self.inner.lock().unwrap();
        inner.ignore_cr = ignore_cr;
    }

    pub fn map_cr_to_lf(&self) -> bool {
        self.inner.lock().unwrap().map_cr_to_lf
    }

    pub fn set_map_cr_to_lf(&self, map_cr_to_lf: bool) {
        let mut inner = self.inner.lock().unwrap();
        inner.map_cr_to_lf = map_cr_to_lf;
    }

    pub fn map_lf_to_cr(&self) -> bool {
        self.inner.lock().unwrap().map_lf_to_cr
    }

    pub fn set_map_lf_to_cr(&self, map_lf_to_cr: bool) {
        let mut inner = self.inner.lock().unwrap();
        inner.map_lf_to_cr = map_lf_to_cr;
    }
}

#[derive(Debug)]
pub struct Tty {
    stdin: Box<dyn VirtualFile + Send + Sync + 'static>,
    stdout: Box<dyn VirtualFile + Send + Sync + 'static>,
    signaler: Option<Box<dyn SignalHandlerAbi + Send + Sync + 'static>>,
    is_mobile: bool,
    last: Option<(String, u128)>,
    options: TtyOptions,
    parser: InputParser,
    line: LineDiscipline,
}

#[derive(Debug, Default)]
struct LineDiscipline {
    chars: Vec<char>,
    cursor: usize,
}

impl LineDiscipline {
    fn len(&self) -> usize {
        self.chars.len()
    }

    fn cursor(&self) -> usize {
        self.cursor
    }

    fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }

    fn insert_text(&mut self, text: &str) {
        for ch in text.chars() {
            self.chars.insert(self.cursor, ch);
            self.cursor += 1;
        }
    }

    fn backspace(&mut self) -> bool {
        if self.cursor == 0 {
            return false;
        }
        self.cursor -= 1;
        self.chars.remove(self.cursor);
        true
    }

    fn clear(&mut self) {
        self.chars.clear();
        self.cursor = 0;
    }

    fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    fn move_right(&mut self) {
        if self.cursor < self.chars.len() {
            self.cursor += 1;
        }
    }

    fn home(&mut self) {
        self.cursor = 0;
    }

    fn end(&mut self) {
        self.cursor = self.chars.len();
    }

    fn ctrl_u(&mut self) -> usize {
        let removed = self.chars.len();
        self.clear();
        removed
    }

    // Erase the whitespace run immediately before the cursor, then the preceding
    // non-whitespace run. For example, with "foo_bar baz" and the cursor at EOL,
    // the first ctrl-w removes "baz" and the next removes "foo_bar".
    fn ctrl_w(&mut self) -> usize {
        // TODO: This currently uses whitespace boundaries.
        // Linux n_tty ALTWERASE semantics are closer to [A-Za-z0-9_] word classes.
        let start = self.chars.len();
        while self.cursor > 0 && self.chars[self.cursor - 1].is_whitespace() {
            self.cursor -= 1;
            self.chars.remove(self.cursor);
        }
        while self.cursor > 0 && !self.chars[self.cursor - 1].is_whitespace() {
            self.cursor -= 1;
            self.chars.remove(self.cursor);
        }
        start.saturating_sub(self.chars.len())
    }

    fn take_line(&mut self) -> String {
        let line: String = self.chars.iter().collect();
        self.clear();
        line
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ParsedInput {
    Text(String),
    Enter,
    Eof,
    CtrlC,
    CtrlBackslash,
    CtrlZ,
    Backspace,
    CtrlU,
    CtrlW,
    CursorLeft,
    CursorRight,
    CursorUp,
    CursorDown,
    Home,
    End,
    CtrlL,
    Tab,
    PageUp,
    PageDown,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum EscapeMatch {
    Prefix,
    Invalid,
    Complete(ParsedInput),
}

const KNOWN_ESCAPE_SEQUENCES: [(&[u8], ParsedInput); 28] = [
    (b"\x1b[D", ParsedInput::CursorLeft),
    (b"\x1b[C", ParsedInput::CursorRight),
    (b"\x1b[A", ParsedInput::CursorUp),
    (b"\x1b[B", ParsedInput::CursorDown),
    (b"\x1bOD", ParsedInput::CursorLeft),
    (b"\x1bOC", ParsedInput::CursorRight),
    (b"\x1bOA", ParsedInput::CursorUp),
    (b"\x1bOB", ParsedInput::CursorDown),
    (b"\x1b[H", ParsedInput::Home),
    (b"\x1b[F", ParsedInput::End),
    (b"\x1b[1~", ParsedInput::Home),
    (b"\x1b[4~", ParsedInput::End),
    (b"\x1b[7~", ParsedInput::Home),
    (b"\x1b[8~", ParsedInput::End),
    (b"\x1b[5~", ParsedInput::PageUp),
    (b"\x1b[6~", ParsedInput::PageDown),
    (b"\x1bOP", ParsedInput::F1),
    (b"\x1bOQ", ParsedInput::F2),
    (b"\x1bOR", ParsedInput::F3),
    (b"\x1bOS", ParsedInput::F4),
    (b"\x1b[15~", ParsedInput::F5),
    (b"\x1b[17~", ParsedInput::F6),
    (b"\x1b[18~", ParsedInput::F7),
    (b"\x1b[19~", ParsedInput::F8),
    (b"\x1b[20~", ParsedInput::F9),
    (b"\x1b[21~", ParsedInput::F10),
    (b"\x1b[23~", ParsedInput::F11),
    (b"\x1b[24~", ParsedInput::F12),
];

#[derive(Debug, Default)]
struct InputParser {
    // Bytes of an in-flight escape sequence so CSI/SS3 fragments can be matched
    // across separate websocket or PTY frames.
    esc_buf: Vec<u8>,
    // Trailing bytes of an incomplete UTF-8 codepoint, replayed into the next
    // chunk before decoding plain text.
    utf8_buf: Vec<u8>,
    // When CR is mapped to LF, suppress the LF in a following CRLF pair so the
    // parser emits only one Enter event.
    pending_lf_after_cr: bool,
}

#[derive(Debug, Clone, Copy, Default)]
struct InputParserConfig {
    ignore_cr: bool,
    map_cr_to_lf: bool,
    map_lf_to_cr: bool,
}

impl InputParser {
    fn reset(&mut self) {
        self.esc_buf.clear();
        self.utf8_buf.clear();
        self.pending_lf_after_cr = false;
    }

    fn match_escape(seq: &[u8]) -> EscapeMatch {
        for (known, parsed) in KNOWN_ESCAPE_SEQUENCES {
            if known == seq {
                return EscapeMatch::Complete(parsed);
            }
            if known.starts_with(seq) {
                return EscapeMatch::Prefix;
            }
        }
        EscapeMatch::Invalid
    }

    fn flush_plain(
        &mut self,
        plain: &mut Vec<u8>,
        out: &mut Vec<ParsedInput>,
        allow_incomplete_utf8: bool,
    ) {
        if plain.is_empty() {
            return;
        }

        match std::str::from_utf8(plain) {
            Ok(s) => out.push(ParsedInput::Text(s.to_string())),
            Err(err) => {
                let valid_up_to = err.valid_up_to();
                if valid_up_to > 0 {
                    out.push(ParsedInput::Text(
                        String::from_utf8_lossy(&plain[..valid_up_to]).into_owned(),
                    ));
                }

                let tail = &plain[valid_up_to..];
                if !tail.is_empty() {
                    if err.error_len().is_none() && allow_incomplete_utf8 {
                        self.utf8_buf.extend_from_slice(tail);
                    } else {
                        out.push(ParsedInput::Text(
                            String::from_utf8_lossy(tail).into_owned(),
                        ));
                    }
                }
            }
        }
        plain.clear();
    }

    fn feed(&mut self, input: &[u8], config: InputParserConfig) -> Vec<ParsedInput> {
        let mut out = Vec::new();
        let mut plain = Vec::new();
        if !self.utf8_buf.is_empty() {
            plain.extend_from_slice(&self.utf8_buf);
            self.utf8_buf.clear();
        }

        for &input_byte in input {
            let mut byte = input_byte;
            loop {
                if self.pending_lf_after_cr {
                    self.pending_lf_after_cr = false;
                    if byte == b'\n' {
                        break;
                    }
                }

                if !self.esc_buf.is_empty() {
                    self.esc_buf.push(byte);
                    match Self::match_escape(&self.esc_buf) {
                        EscapeMatch::Prefix => break,
                        EscapeMatch::Complete(parsed) => {
                            self.flush_plain(&mut plain, &mut out, false);
                            self.esc_buf.clear();
                            out.push(parsed);
                            break;
                        }
                        EscapeMatch::Invalid => {
                            let Some(last) = self.esc_buf.pop() else {
                                break;
                            };
                            plain.extend_from_slice(&self.esc_buf);
                            self.esc_buf.clear();
                            byte = last;
                            continue;
                        }
                    }
                }

                let mut mapped = byte;
                if byte == b'\r' {
                    if config.ignore_cr {
                        break;
                    }
                    if config.map_cr_to_lf {
                        mapped = b'\n';
                    }
                } else if byte == b'\n' && config.map_lf_to_cr {
                    mapped = b'\r';
                }

                match mapped {
                    b'\x1B' => {
                        self.flush_plain(&mut plain, &mut out, false);
                        self.esc_buf.push(mapped);
                    }
                    b'\n' => {
                        self.flush_plain(&mut plain, &mut out, false);
                        if byte == b'\r' && config.map_cr_to_lf {
                            self.pending_lf_after_cr = true;
                        }
                        out.push(ParsedInput::Enter);
                    }
                    0x04 => {
                        self.flush_plain(&mut plain, &mut out, false);
                        out.push(ParsedInput::Eof);
                    }
                    0x03 => {
                        self.flush_plain(&mut plain, &mut out, false);
                        out.push(ParsedInput::CtrlC);
                    }
                    0x1C => {
                        self.flush_plain(&mut plain, &mut out, false);
                        out.push(ParsedInput::CtrlBackslash);
                    }
                    0x1A => {
                        self.flush_plain(&mut plain, &mut out, false);
                        out.push(ParsedInput::CtrlZ);
                    }
                    0x08 | 0x7F => {
                        self.flush_plain(&mut plain, &mut out, false);
                        out.push(ParsedInput::Backspace);
                    }
                    0x09 => {
                        self.flush_plain(&mut plain, &mut out, false);
                        out.push(ParsedInput::Tab);
                    }
                    0x15 => {
                        self.flush_plain(&mut plain, &mut out, false);
                        out.push(ParsedInput::CtrlU);
                    }
                    0x17 => {
                        self.flush_plain(&mut plain, &mut out, false);
                        out.push(ParsedInput::CtrlW);
                    }
                    0x01 => {
                        self.flush_plain(&mut plain, &mut out, false);
                        out.push(ParsedInput::Home);
                    }
                    0x0C => {
                        self.flush_plain(&mut plain, &mut out, false);
                        out.push(ParsedInput::CtrlL);
                    }
                    _ => plain.push(mapped),
                }
                break;
            }
        }

        self.flush_plain(&mut plain, &mut out, true);
        out
    }
}

impl Tty {
    async fn signal_and_clear_line(&mut self, signal: Option<Signal>, echo: bool) {
        if let (Some(signaler), Some(signal)) = (self.signaler.as_ref(), signal) {
            signaler.signal(signal as u8).ok();
        }
        self.line.clear();
        if echo {
            self.write_stdout(b"\n").await;
        }
    }

    pub fn new(
        stdin: Box<dyn VirtualFile + Send + Sync + 'static>,
        stdout: Box<dyn VirtualFile + Send + Sync + 'static>,
        is_mobile: bool,
        options: TtyOptions,
    ) -> Self {
        Self {
            stdin,
            stdout,
            signaler: None,
            last: None,
            options,
            is_mobile,
            parser: InputParser::default(),
            line: LineDiscipline::default(),
        }
    }

    pub fn stdin(&self) -> &(dyn VirtualFile + Send + Sync + 'static) {
        self.stdin.as_ref()
    }

    pub fn stdin_mut(&mut self) -> &mut (dyn VirtualFile + Send + Sync + 'static) {
        self.stdin.as_mut()
    }

    pub fn stdin_replace(
        &mut self,
        mut stdin: Box<dyn VirtualFile + Send + Sync + 'static>,
    ) -> Box<dyn VirtualFile + Send + Sync + 'static> {
        std::mem::swap(&mut self.stdin, &mut stdin);
        stdin
    }

    pub fn stdin_take(&mut self) -> Box<dyn VirtualFile + Send + Sync + 'static> {
        let mut stdin: Box<dyn VirtualFile + Send + Sync + 'static> = Box::<NullFile>::default();
        std::mem::swap(&mut self.stdin, &mut stdin);
        stdin
    }

    pub fn options(&self) -> TtyOptions {
        self.options.clone()
    }

    pub fn set_signaler(&mut self, signaler: Box<dyn SignalHandlerAbi + Send + Sync + 'static>) {
        self.signaler.replace(signaler);
    }

    pub fn on_event(mut self, event: InputEvent) -> BoxFuture<'static, Self> {
        Box::pin(async move {
            match event {
                InputEvent::Key => {
                    // do nothing
                    self
                }
                InputEvent::Data(data) => {
                    // Due to a nasty bug in xterm.js on Android mobile it sends the keys you press
                    // twice in a row with a short interval between - this hack will avoid that bug
                    if self.is_mobile {
                        let now = platform_clock_time_get(Snapshot0Clockid::Monotonic, 1_000_000)
                            .unwrap() as u128;
                        if let Some((what, when)) = self.last.as_ref()
                            && what.as_str() == data
                            && now - *when < TTY_MOBILE_PAUSE
                        {
                            self.last = None;
                            return self;
                        }
                        self.last = Some((data.clone(), now))
                    }
                    self.on_data(data.into_bytes()).await
                }
                InputEvent::Raw(data) => self.on_data(data).await,
            }
        })
    }

    async fn write_stdout(&mut self, bytes: &[u8]) {
        let _ = self.stdout.write(bytes).await;
    }

    async fn write_stdin(&mut self, bytes: &[u8]) {
        let _ = self.stdin.write(bytes).await;
    }

    async fn apply_canonical_input(&mut self, input: ParsedInput, echo: bool) {
        match input {
            ParsedInput::Text(text) => {
                let old_cursor = self.line.cursor();
                let old_len = self.line.len();
                if echo {
                    self.write_stdout(text.as_bytes()).await;
                }
                self.line.insert_text(&text);
                if echo && old_cursor < old_len {
                    let tail_start = old_cursor + text.chars().count();
                    let tail: String = self.line.chars[tail_start..].iter().collect();
                    if !tail.is_empty() {
                        self.write_stdout(tail.as_bytes()).await;
                        for _ in 0..tail.chars().count() {
                            self.write_stdout(b"\x08").await;
                        }
                    }
                }
            }
            ParsedInput::Enter => {
                let mut data = self.line.take_line();
                data.push('\n');
                if echo {
                    self.write_stdout(b"\n").await;
                }
                self.write_stdin(data.as_bytes()).await;
            }
            ParsedInput::Eof => {
                if !self.line.is_empty() {
                    let data = self.line.take_line();
                    self.write_stdin(data.as_bytes()).await;
                }
            }
            ParsedInput::CtrlC => {
                self.signal_and_clear_line(Some(Signal::Sigint), echo).await;
            }
            ParsedInput::CtrlBackslash => {
                self.signal_and_clear_line(Some(Signal::Sigquit), echo)
                    .await;
            }
            ParsedInput::CtrlZ => {
                self.signal_and_clear_line(Some(Signal::Sigtstp), echo)
                    .await;
            }
            ParsedInput::Backspace => {
                let old_cursor = self.line.cursor();
                let old_len = self.line.len();
                if self.line.backspace() && echo {
                    if old_cursor < old_len {
                        let tail: String = self.line.chars[self.line.cursor()..].iter().collect();
                        self.write_stdout(tail.as_bytes()).await;
                        self.write_stdout(b" ").await;
                        for _ in 0..(tail.chars().count() + 1) {
                            self.write_stdout(b"\x08").await;
                        }
                    } else {
                        self.write_stdout("\u{0008} \u{0008}".as_bytes()).await;
                    }
                }
            }
            ParsedInput::CtrlU => {
                let removed = self.line.ctrl_u();
                if echo {
                    for _ in 0..removed {
                        self.write_stdout("\u{0008} \u{0008}".as_bytes()).await;
                    }
                }
            }
            ParsedInput::CtrlW => {
                let removed = self.line.ctrl_w();
                if echo {
                    for _ in 0..removed {
                        self.write_stdout("\u{0008} \u{0008}".as_bytes()).await;
                    }
                }
            }
            ParsedInput::CursorLeft => self.line.move_left(),
            ParsedInput::CursorRight => self.line.move_right(),
            ParsedInput::Home => self.line.home(),
            ParsedInput::End => self.line.end(),
            ParsedInput::CursorUp
            | ParsedInput::CursorDown
            | ParsedInput::CtrlL
            | ParsedInput::Tab
            | ParsedInput::PageUp
            | ParsedInput::PageDown
            | ParsedInput::F1
            | ParsedInput::F2
            | ParsedInput::F3
            | ParsedInput::F4
            | ParsedInput::F5
            | ParsedInput::F6
            | ParsedInput::F7
            | ParsedInput::F8
            | ParsedInput::F9
            | ParsedInput::F10
            | ParsedInput::F11
            | ParsedInput::F12 => {}
        }
    }

    fn on_data(mut self, data: Vec<u8>) -> BoxFuture<'static, Self> {
        let options = { self.options.inner.lock().unwrap().clone() };
        if options.line_buffering {
            let parser_config = InputParserConfig {
                ignore_cr: options.ignore_cr,
                map_cr_to_lf: options.map_cr_to_lf,
                map_lf_to_cr: options.map_lf_to_cr,
            };
            let parsed_inputs = self.parser.feed(&data, parser_config);
            return Box::pin(async move {
                for input in parsed_inputs {
                    self.apply_canonical_input(input, options.echo).await;
                }
                self
            });
        };

        self.parser.reset();
        Box::pin(async move {
            if options.echo {
                self.write_stdout(&data).await;
            }
            self.write_stdin(&data).await;
            self
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct WasiTtyState {
    pub cols: u32,
    pub rows: u32,
    pub width: u32,
    pub height: u32,
    pub stdin_tty: bool,
    pub stdout_tty: bool,
    pub stderr_tty: bool,
    pub echo: bool,
    pub line_buffered: bool,
    pub line_feeds: bool,
}

impl Default for WasiTtyState {
    fn default() -> Self {
        Self {
            cols: 80,
            rows: 25,
            width: 800,
            height: 600,
            stdin_tty: true,
            stdout_tty: true,
            stderr_tty: true,
            echo: false,
            line_buffered: false,
            line_feeds: true,
        }
    }
}

/// Provides access to a TTY.
pub trait TtyBridge: std::fmt::Debug {
    /// Resets the values
    fn reset(&self);

    /// Retrieve the current TTY state.
    fn tty_get(&self) -> WasiTtyState;

    /// Set the TTY state.
    fn tty_set(&self, _tty_state: WasiTtyState);
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Seek, Write},
        pin::Pin,
        sync::{Arc, Mutex},
        task::{Context, Poll},
    };

    use tokio::io::{AsyncRead, AsyncSeek, AsyncWrite};
    use virtual_fs::VirtualFile as VirtualFileTrait;
    use virtual_mio::block_on;
    use wasmer_wasix_types::wasi::Signal;

    use super::{InputEvent, Tty, TtyOptions, WasiTtyState};
    use crate::os::task::signal::{SignalDeliveryError, SignalHandlerAbi};

    #[derive(Debug)]
    struct CaptureFile {
        buffer: Arc<Mutex<Vec<u8>>>,
    }

    impl CaptureFile {
        fn new(buffer: Arc<Mutex<Vec<u8>>>) -> Self {
            Self { buffer }
        }
    }

    impl VirtualFileTrait for CaptureFile {
        fn last_accessed(&self) -> u64 {
            0
        }

        fn last_modified(&self) -> u64 {
            0
        }

        fn created_time(&self) -> u64 {
            0
        }

        fn size(&self) -> u64 {
            self.buffer.lock().unwrap().len() as u64
        }

        fn set_len(&mut self, _new_size: u64) -> Result<(), virtual_fs::FsError> {
            Err(virtual_fs::FsError::PermissionDenied)
        }

        fn unlink(&mut self) -> Result<(), virtual_fs::FsError> {
            Ok(())
        }

        fn is_open(&self) -> bool {
            true
        }

        fn get_special_fd(&self) -> Option<u32> {
            None
        }

        fn poll_read_ready(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<std::io::Result<usize>> {
            Poll::Ready(Ok(0))
        }

        fn poll_write_ready(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<std::io::Result<usize>> {
            Poll::Ready(Ok(8192))
        }
    }

    impl AsyncRead for CaptureFile {
        fn poll_read(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _buf: &mut tokio::io::ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    impl AsyncWrite for CaptureFile {
        fn poll_write(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            Poll::Ready(self.write(buf))
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    impl AsyncSeek for CaptureFile {
        fn start_seek(self: Pin<&mut Self>, _position: std::io::SeekFrom) -> std::io::Result<()> {
            Ok(())
        }

        fn poll_complete(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<std::io::Result<u64>> {
            Poll::Ready(Ok(0))
        }
    }

    impl Read for CaptureFile {
        fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
            Ok(0)
        }
    }

    impl Write for CaptureFile {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let mut buffer = self.buffer.lock().unwrap();
            buffer.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Seek for CaptureFile {
        fn seek(&mut self, _pos: std::io::SeekFrom) -> std::io::Result<u64> {
            Ok(0)
        }
    }

    #[derive(Debug)]
    struct RecordingSignaler {
        signals: Arc<Mutex<Vec<u8>>>,
    }

    impl RecordingSignaler {
        fn new(signals: Arc<Mutex<Vec<u8>>>) -> Self {
            Self { signals }
        }
    }

    impl SignalHandlerAbi for RecordingSignaler {
        fn signal(&self, signal: u8) -> Result<(), SignalDeliveryError> {
            self.signals.lock().unwrap().push(signal);
            Ok(())
        }
    }

    fn captured(buffer: &Arc<Mutex<Vec<u8>>>) -> String {
        String::from_utf8(buffer.lock().unwrap().clone()).unwrap()
    }

    fn new_tty(
        echo: bool,
        line_buffering: bool,
    ) -> (Tty, Arc<Mutex<Vec<u8>>>, Arc<Mutex<Vec<u8>>>) {
        new_tty_with_mobile(echo, line_buffering, false)
    }

    fn new_tty_with_mobile(
        echo: bool,
        line_buffering: bool,
        is_mobile: bool,
    ) -> (Tty, Arc<Mutex<Vec<u8>>>, Arc<Mutex<Vec<u8>>>) {
        let stdin_buffer = Arc::new(Mutex::new(Vec::new()));
        let stdout_buffer = Arc::new(Mutex::new(Vec::new()));

        let options = TtyOptions::default();
        options.set_echo(echo);
        options.set_line_buffering(line_buffering);

        let tty = Tty::new(
            Box::new(CaptureFile::new(stdin_buffer.clone())),
            Box::new(CaptureFile::new(stdout_buffer.clone())),
            is_mobile,
            options,
        );

        (tty, stdin_buffer, stdout_buffer)
    }

    fn run_event(tty: Tty, event: InputEvent) -> Tty {
        block_on(tty.on_event(event))
    }

    fn run_events(mut tty: Tty, events: Vec<InputEvent>) -> Tty {
        for event in events {
            tty = run_event(tty, event);
        }
        tty
    }

    #[test]
    fn tty_canonical_enter_flushes_line_to_stdin() {
        let (tty, stdin_buf, stdout_buf) = new_tty(true, true);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("pwd".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect canonical mode to flush the buffered line to stdin and echo it to stdout.
        assert_eq!(captured(&stdin_buf), "pwd\n");
        assert_eq!(captured(&stdout_buf), "pwd\n");
    }

    #[test]
    fn tty_canonical_lf_is_enter() {
        let (tty, stdin_buf, stdout_buf) = new_tty(true, true);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("pwd".to_string()),
                InputEvent::Data("\n".to_string()),
            ],
        );

        // Expect LF to be treated as Enter in canonical mode.
        assert_eq!(captured(&stdin_buf), "pwd\n");
        assert_eq!(captured(&stdout_buf), "pwd\n");
    }

    #[test]
    fn tty_canonical_echo_disabled_still_forwards_line() {
        let (tty, stdin_buf, stdout_buf) = new_tty(false, true);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("pwd".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect line forwarding to stdin, but no echo when echo is disabled.
        assert_eq!(captured(&stdin_buf), "pwd\n");
        assert_eq!(captured(&stdout_buf), "");
    }

    #[test]
    fn tty_canonical_backspace_removes_last_ascii_char() {
        let (tty, stdin_buf, stdout_buf) = new_tty(true, true);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("ab".to_string()),
                InputEvent::Data("\u{007F}".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect DEL to remove one character from buffered input and emit erase echo sequence.
        assert_eq!(captured(&stdin_buf), "a\n");
        assert_eq!(
            captured(&stdout_buf),
            format!("ab{}\n", "\u{0008} \u{0008}")
        );
    }

    #[test]
    fn tty_canonical_backspace_on_empty_line_is_noop() {
        let (tty, stdin_buf, stdout_buf) = new_tty(true, true);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("\u{007F}".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect backspace on an empty line to be ignored.
        assert_eq!(captured(&stdin_buf), "\n");
        assert_eq!(captured(&stdout_buf), "\n");
    }

    #[test]
    fn tty_ctrl_c_signals_and_clears_buffered_line() {
        let (mut tty, stdin_buf, stdout_buf) = new_tty(true, true);
        let signals = Arc::new(Mutex::new(Vec::new()));
        tty.set_signaler(Box::new(RecordingSignaler::new(signals.clone())));

        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("abc".to_string()),
                InputEvent::Data("\u{0003}".to_string()),
                InputEvent::Data("x".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect ctrl-c to clear current line, then accept/forward subsequent input and emit SIGINT.
        assert_eq!(captured(&stdin_buf), "x\n");
        assert_eq!(captured(&stdout_buf), "abc\nx\n");
        assert_eq!(signals.lock().unwrap().as_slice(), &[Signal::Sigint as u8]);
    }

    #[test]
    fn tty_ctrl_c_without_signaler_clears_buffer_and_echoes_newline() {
        let (tty, stdin_buf, stdout_buf) = new_tty(true, true);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("abc".to_string()),
                InputEvent::Data("\u{0003}".to_string()),
                InputEvent::Data("x".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect ctrl-c to clear current input line and continue with subsequent input.
        assert_eq!(captured(&stdin_buf), "x\n");
        assert_eq!(captured(&stdout_buf), "abc\nx\n");
    }

    #[test]
    fn tty_special_keys_do_not_edit_or_forward_by_default() {
        let (tty, stdin_buf, stdout_buf) = new_tty(true, true);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("\u{001B}\u{005B}\u{0044}".to_string()), // left
                InputEvent::Data("\u{001B}\u{005B}\u{0043}".to_string()), // right
                InputEvent::Data("\u{001B}\u{005B}\u{0041}".to_string()), // up
                InputEvent::Data("\u{001B}\u{005B}\u{0042}".to_string()), // down
                InputEvent::Data("a".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect currently-implemented navigation keys to be consumed without mutating line state.
        assert_eq!(captured(&stdin_buf), "a\n");
        assert_eq!(captured(&stdout_buf), "a\n");
    }

    #[test]
    fn tty_tab_is_consumed_without_forwarding() {
        let (tty, stdin_buf, stdout_buf) = new_tty(true, true);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("\u{0009}".to_string()),
                InputEvent::Data("a".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect tab key to be consumed by handler and not forwarded into the line buffer.
        assert_eq!(captured(&stdin_buf), "a\n");
        assert_eq!(captured(&stdout_buf), "a\n");
    }

    #[test]
    fn tty_key_event_is_noop() {
        let (tty, stdin_buf, stdout_buf) = new_tty(true, true);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Key,
                InputEvent::Data("x".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect generic Key events to be no-op and not affect data flow.
        assert_eq!(captured(&stdin_buf), "x\n");
        assert_eq!(captured(&stdout_buf), "x\n");
    }

    #[test]
    fn tty_extended_navigation_and_function_keys_are_consumed() {
        let (tty, stdin_buf, stdout_buf) = new_tty(true, true);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("\u{0001}".to_string()), // ctrl-a (home)
                InputEvent::Data("\u{001B}\u{005B}\u{0048}".to_string()), // home
                InputEvent::Data("\u{001B}\u{005B}\u{0046}".to_string()), // end
                InputEvent::Data("\u{000C}".to_string()), // ctrl-l
                InputEvent::Data("\u{001B}\u{005B}\u{0035}\u{007E}".to_string()), // page up
                InputEvent::Data("\u{001B}\u{005B}\u{0036}\u{007E}".to_string()), // page down
                InputEvent::Data("\u{001B}\u{004F}\u{0050}".to_string()), // f1
                InputEvent::Data("\u{001B}\u{004F}\u{0051}".to_string()), // f2
                InputEvent::Data("\u{001B}\u{004F}\u{0052}".to_string()), // f3
                InputEvent::Data("\u{001B}\u{004F}\u{0053}".to_string()), // f4
                InputEvent::Data("\u{001B}\u{005B}\u{0031}\u{0035}\u{007E}".to_string()), // f5
                InputEvent::Data("\u{001B}\u{005B}\u{0031}\u{0037}\u{007E}".to_string()), // f6
                InputEvent::Data("\u{001B}\u{005B}\u{0031}\u{0038}\u{007E}".to_string()), // f7
                InputEvent::Data("\u{001B}\u{005B}\u{0031}\u{0039}\u{007E}".to_string()), // f8
                InputEvent::Data("\u{001B}\u{005B}\u{0032}\u{0030}\u{007E}".to_string()), // f9
                InputEvent::Data("\u{001B}\u{005B}\u{0032}\u{0031}\u{007E}".to_string()), // f10
                InputEvent::Data("\u{001B}\u{005B}\u{0032}\u{0033}\u{007E}".to_string()), // f11
                InputEvent::Data("\u{001B}\u{005B}\u{0032}\u{0034}\u{007E}".to_string()), // f12
                InputEvent::Data("z".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect recognized extended key sequences to be consumed and regular input to remain intact.
        assert_eq!(captured(&stdin_buf), "z\n");
        assert_eq!(captured(&stdout_buf), "z\n");
    }

    #[test]
    fn tty_canonical_multiple_lines_do_not_bleed_into_each_other() {
        let (tty, stdin_buf, stdout_buf) = new_tty(true, true);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("one".to_string()),
                InputEvent::Data("\r".to_string()),
                InputEvent::Data("two".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect line buffer reset after each Enter with no cross-line bleed.
        assert_eq!(captured(&stdin_buf), "one\ntwo\n");
        assert_eq!(captured(&stdout_buf), "one\ntwo\n");
    }

    #[test]
    fn tty_raw_mode_forwards_without_line_buffering() {
        let (tty, stdin_buf, stdout_buf) = new_tty(false, false);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("pwd".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect raw mode to pass bytes through unchanged and skip echo when echo is disabled.
        assert_eq!(captured(&stdin_buf), "pwd\r");
        assert_eq!(captured(&stdout_buf), "");
    }

    #[test]
    fn tty_raw_mode_can_echo() {
        let (tty, stdin_buf, stdout_buf) = new_tty(true, false);
        let _tty = run_events(tty, vec![InputEvent::Data("raw".to_string())]);

        // Expect raw mode with echo enabled to mirror exactly what is forwarded.
        assert_eq!(captured(&stdin_buf), "raw");
        assert_eq!(captured(&stdout_buf), "raw");
    }

    #[test]
    fn tty_raw_mode_backspace_is_forwarded() {
        let (tty, stdin_buf, stdout_buf) = new_tty(true, false);
        let _tty = run_events(tty, vec![InputEvent::Data("\u{007F}".to_string())]);

        // Expect DEL to be forwarded literally in raw mode.
        assert_eq!(captured(&stdin_buf), "\u{007F}");
        assert_eq!(captured(&stdout_buf), "\u{007F}");
    }

    #[test]
    fn tty_raw_mode_escape_sequence_is_forwarded() {
        let (tty, stdin_buf, stdout_buf) = new_tty(true, false);
        let _tty = run_events(
            tty,
            vec![InputEvent::Data("\u{001B}\u{005B}\u{0044}".to_string())],
        );

        // Expect escape bytes to be forwarded literally in raw mode.
        assert_eq!(captured(&stdin_buf), "\u{001B}\u{005B}\u{0044}");
        assert_eq!(captured(&stdout_buf), "\u{001B}\u{005B}\u{0044}");
    }

    #[test]
    fn tty_raw_input_event_behaves_like_data_input_event() {
        let (tty, stdin_buf, stdout_buf) = new_tty(true, false);
        let _tty = run_events(tty, vec![InputEvent::Raw(b"xyz".to_vec())]);

        // Expect InputEvent::Raw to follow the same raw-mode path as InputEvent::Data.
        assert_eq!(captured(&stdin_buf), "xyz");
        assert_eq!(captured(&stdout_buf), "xyz");
    }

    #[test]
    fn tty_canonical_utf8_single_chunk_roundtrip() {
        let (tty, stdin_buf, stdout_buf) = new_tty(true, true);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("hé".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect UTF-8 characters in one chunk to roundtrip in canonical mode.
        assert_eq!(captured(&stdin_buf), "hé\n");
        assert_eq!(captured(&stdout_buf), "hé\n");
    }

    #[test]
    fn tty_consecutive_enters_emit_empty_lines() {
        let (tty, stdin_buf, stdout_buf) = new_tty(true, true);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("\r".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect each Enter to emit an empty line independently.
        assert_eq!(captured(&stdin_buf), "\n\n");
        assert_eq!(captured(&stdout_buf), "\n\n");
    }

    #[test]
    fn tty_stdin_replace_redirects_future_writes() {
        let (mut tty, stdin_buf_a, _) = new_tty(false, true);
        let stdin_buf_b = Arc::new(Mutex::new(Vec::new()));
        let _old = tty.stdin_replace(Box::new(CaptureFile::new(stdin_buf_b.clone())));

        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("new".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect writes to go only to replaced stdin target after stdin_replace.
        assert_eq!(captured(&stdin_buf_a), "");
        assert_eq!(captured(&stdin_buf_b), "new\n");
    }

    #[test]
    fn tty_unknown_escape_sequence_is_buffered_as_data() {
        let (tty, stdin_buf, stdout_buf) = new_tty(true, true);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("\u{001B}[X".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect unknown escape sequence bytes to be treated as regular input.
        assert_eq!(captured(&stdin_buf), "\u{001B}[X\n");
        assert_eq!(captured(&stdout_buf), "\u{001B}[X\n");
    }

    #[test]
    fn tty_raw_mode_ctrl_c_is_forwarded_as_data() {
        let (tty, stdin_buf, stdout_buf) = new_tty(true, false);
        let _tty = run_events(tty, vec![InputEvent::Data("\u{0003}".to_string())]);

        // Expect ctrl-c byte passthrough in raw mode.
        assert_eq!(captured(&stdin_buf), "\u{0003}");
        assert_eq!(captured(&stdout_buf), "\u{0003}");
    }

    #[test]
    fn tty_mobile_duplicate_data_is_suppressed() {
        let (tty, stdin_buf, stdout_buf) = new_tty_with_mobile(true, true, true);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("x".to_string()),
                InputEvent::Data("x".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect duplicate suppression on mobile path for identical near-consecutive input.
        assert_eq!(captured(&stdin_buf), "x\n");
        assert_eq!(captured(&stdout_buf), "x\n");
    }

    #[test]
    fn tty_non_mobile_duplicate_data_is_not_suppressed() {
        let (tty, stdin_buf, stdout_buf) = new_tty_with_mobile(true, true, false);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("x".to_string()),
                InputEvent::Data("x".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect no deduplication outside mobile path.
        assert_eq!(captured(&stdin_buf), "xx\n");
        assert_eq!(captured(&stdout_buf), "xx\n");
    }

    #[test]
    fn tty_chunk_split_command_plus_enter_flushes() {
        let cases = vec![
            vec!["echo hello", "\r"],
            vec!["echo ", "hello", "\r"],
            vec!["e", "cho hello", "\r"],
        ];

        for chunks in cases {
            let (tty, stdin_buf, _) = new_tty(false, true);
            let events = chunks
                .into_iter()
                .map(|chunk| InputEvent::Data(chunk.to_string()))
                .collect::<Vec<_>>();
            let _tty = run_events(tty, events);
            // Expect split delivery across chunks to preserve command+enter behavior.
            assert_eq!(captured(&stdin_buf), "echo hello\n");
        }
    }

    #[test]
    fn tty_single_frame_command_plus_enter_is_executed() {
        let (tty, stdin_buf, _) = new_tty(false, true);
        let _tty = run_events(tty, vec![InputEvent::Data("echo hello\r".to_string())]);

        // Expect one-frame command+enter to execute as a full line.
        assert_eq!(captured(&stdin_buf), "echo hello\n");
    }

    #[test]
    fn tty_utf8_backspace_removes_full_character() {
        let (tty, stdin_buf, _) = new_tty(false, true);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("é".to_string()),
                InputEvent::Data("\u{007F}".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect backspace to remove an entire UTF-8 character.
        assert_eq!(captured(&stdin_buf), "\n");
    }

    #[test]
    fn tty_crlf_single_chunk_is_treated_as_one_enter() {
        let (tty, stdin_buf, _) = new_tty(false, true);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("pwd".to_string()),
                InputEvent::Data("\r\n".to_string()),
            ],
        );

        // Expect CRLF in one chunk to be normalized as a single Enter.
        assert_eq!(captured(&stdin_buf), "pwd\n");
    }

    #[test]
    fn tty_cr_then_lf_split_is_single_enter() {
        let (tty, stdin_buf, _) = new_tty(false, true);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("pwd".to_string()),
                InputEvent::Data("\r".to_string()),
                InputEvent::Data("\n".to_string()),
            ],
        );

        // Expect split CR then LF to still map to a single Enter event.
        assert_eq!(captured(&stdin_buf), "pwd\n");
    }

    #[test]
    fn tty_split_left_arrow_escape_sequence_is_consumed() {
        let (tty, stdin_buf, stdout_buf) = new_tty(true, true);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("\u{001B}".to_string()),
                InputEvent::Data("[".to_string()),
                InputEvent::Data("D".to_string()),
                InputEvent::Data("x".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect split escape sequence fragments to be assembled and consumed.
        assert_eq!(captured(&stdin_buf), "x\n");
        assert_eq!(captured(&stdout_buf), "x\n");
    }

    #[test]
    fn tty_split_f5_escape_sequence_is_consumed() {
        let (tty, stdin_buf, stdout_buf) = new_tty(true, true);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("\u{001B}".to_string()),
                InputEvent::Data("[".to_string()),
                InputEvent::Data("15".to_string()),
                InputEvent::Data("~".to_string()),
                InputEvent::Data("x".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect split function-key sequence fragments to be assembled and consumed.
        assert_eq!(captured(&stdin_buf), "x\n");
        assert_eq!(captured(&stdout_buf), "x\n");
    }

    #[test]
    fn tty_left_arrow_moves_cursor_for_inline_insert() {
        let (tty, stdin_buf, _) = new_tty(false, true);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("ab".to_string()),
                InputEvent::Data("\u{001B}\u{005B}\u{0044}".to_string()),
                InputEvent::Data("X".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect cursor-left to enable inline insertion before the final character.
        assert_eq!(captured(&stdin_buf), "aXb\n");
    }

    #[test]
    fn tty_home_key_moves_cursor_to_start() {
        let (tty, stdin_buf, _) = new_tty(false, true);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("bc".to_string()),
                InputEvent::Data("\u{001B}\u{005B}\u{0048}".to_string()),
                InputEvent::Data("a".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect home key to move cursor to start for insertion.
        assert_eq!(captured(&stdin_buf), "abc\n");
    }

    #[test]
    fn tty_ctrl_u_kills_current_line() {
        let (tty, stdin_buf, _) = new_tty(false, true);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("abc".to_string()),
                InputEvent::Data("\u{0015}".to_string()),
                InputEvent::Data("x".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect ctrl-u to kill the current line before new input.
        assert_eq!(captured(&stdin_buf), "x\n");
    }

    #[test]
    fn tty_ctrl_w_erases_previous_word() {
        let (tty, stdin_buf, _) = new_tty(false, true);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("hello world".to_string()),
                InputEvent::Data("\u{0017}".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect ctrl-w to erase the previous word boundary.
        assert_eq!(captured(&stdin_buf), "hello \n");
    }

    #[test]
    fn tty_split_utf8_codepoint_across_raw_chunks() {
        let (tty, stdin_buf, _) = new_tty(false, true);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Raw(vec![0xC3]),
                InputEvent::Raw(vec![0xA9]),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect UTF-8 codepoints split across chunks to be reconstructed correctly.
        assert_eq!(captured(&stdin_buf), "é\n");
    }

    #[test]
    fn tty_ctrl_c_without_signaler_clears_buffer() {
        let (tty, stdin_buf, _) = new_tty(false, true);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("abc".to_string()),
                InputEvent::Data("\u{0003}".to_string()),
                InputEvent::Data("x".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect ctrl-c to clear line buffer even when no external signal handler is registered.
        assert_eq!(captured(&stdin_buf), "x\n");
    }

    #[test]
    fn tty_single_chunk_text_backspace_enter_edits_line() {
        let (tty, stdin_buf, _) = new_tty(false, true);
        let _tty = run_events(tty, vec![InputEvent::Data("ab\u{007F}\r".to_string())]);

        // Expect mixed text+DEL+Enter in one chunk to still honor editing semantics.
        assert_eq!(captured(&stdin_buf), "a\n");
    }

    #[test]
    fn tty_single_chunk_text_ctrlc_enter_clears_line_with_signaler() {
        let (mut tty, stdin_buf, _) = new_tty(false, true);
        let signals = Arc::new(Mutex::new(Vec::new()));
        tty.set_signaler(Box::new(RecordingSignaler::new(signals.clone())));

        let _tty = run_events(tty, vec![InputEvent::Data("ab\u{0003}x\r".to_string())]);

        // Expect mixed text+ctrl-c+enter in one chunk to clear line and still emit SIGINT.
        assert_eq!(captured(&stdin_buf), "x\n");
        assert_eq!(signals.lock().unwrap().as_slice(), &[Signal::Sigint as u8]);
    }

    #[test]
    fn tty_partial_escape_then_enter_preserves_enter_semantics() {
        let (tty, stdin_buf, _) = new_tty(false, true);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("abc".to_string()),
                InputEvent::Data("\u{001B}".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect a lone ESC prefix to be treated as text while Enter still flushes the line.
        assert_eq!(captured(&stdin_buf), "abc\u{001B}\n");
    }

    #[test]
    fn tty_partial_escape_then_ctrl_c_still_interrupts() {
        let (mut tty, stdin_buf, stdout_buf) = new_tty(true, true);
        let signals = Arc::new(Mutex::new(Vec::new()));
        tty.set_signaler(Box::new(RecordingSignaler::new(signals.clone())));

        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("abc".to_string()),
                InputEvent::Data("\u{001B}".to_string()),
                InputEvent::Data("\u{0003}".to_string()),
                InputEvent::Data("x".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect ctrl-c to be handled as interrupt even after a broken ESC prefix.
        assert_eq!(captured(&stdin_buf), "x\n");
        let out = captured(&stdout_buf);
        assert!(out.starts_with("abc"));
        assert!(out.ends_with("\nx\n"));
        assert_eq!(signals.lock().unwrap().as_slice(), &[Signal::Sigint as u8]);
    }

    #[test]
    fn tty_ctrl_d_on_empty_line_is_not_buffered_as_text() {
        let (tty, stdin_buf, stdout_buf) = new_tty(true, true);
        let _tty = run_events(tty, vec![InputEvent::Data("\u{0004}".to_string())]);

        // Expect canonical EOF at BOL to avoid forwarding/echoing literal ctrl-d bytes.
        assert_eq!(captured(&stdin_buf), "");
        assert_eq!(captured(&stdout_buf), "");
    }

    #[test]
    fn tty_ctrl_d_with_buffered_text_flushes_without_newline() {
        let (tty, stdin_buf, stdout_buf) = new_tty(true, true);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("abc".to_string()),
                InputEvent::Data("\u{0004}".to_string()),
            ],
        );

        // Expect canonical EOF with buffered text to flush the buffer without trailing newline.
        assert_eq!(captured(&stdin_buf), "abc");
        assert_eq!(captured(&stdout_buf), "abc");
    }

    #[test]
    fn tty_ctrl_u_echoes_line_erase_feedback() {
        let (tty, stdin_buf, stdout_buf) = new_tty(true, true);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("abc".to_string()),
                InputEvent::Data("\u{0015}".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect ctrl-u to clear the buffered line and echo erase feedback in canonical mode.
        assert_eq!(captured(&stdin_buf), "\n");
        assert!(captured(&stdout_buf).contains("\u{0008} \u{0008}"));
    }

    #[test]
    fn tty_ctrl_w_echoes_word_erase_feedback() {
        let (tty, stdin_buf, stdout_buf) = new_tty(true, true);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("hello world".to_string()),
                InputEvent::Data("\u{0017}".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect ctrl-w to erase the previous word and echo erase feedback.
        assert_eq!(captured(&stdin_buf), "hello \n");
        assert!(captured(&stdout_buf).contains("\u{0008} \u{0008}"));
    }

    #[test]
    fn tty_left_arrow_inline_insert_echoes_cursor_repair() {
        let (tty, stdin_buf, stdout_buf) = new_tty(true, true);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("ab".to_string()),
                InputEvent::Data("\u{001B}\u{005B}\u{0044}".to_string()),
                InputEvent::Data("X".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect inline insertion to preserve data and emit cursor-repair echo output.
        assert_eq!(captured(&stdin_buf), "aXb\n");
        assert!(captured(&stdout_buf).contains('\u{0008}'));
    }

    #[test]
    fn tty_backspace_after_cursor_move_repaints_tail() {
        let (tty, stdin_buf, stdout_buf) = new_tty(true, true);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("ab".to_string()),
                InputEvent::Data("\u{001B}\u{005B}\u{0044}".to_string()),
                InputEvent::Data("\u{007F}".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect deleting in the middle of the line to repaint the remaining tail and restore the cursor.
        assert_eq!(captured(&stdin_buf), "b\n");
        assert_eq!(captured(&stdout_buf), "abb \u{0008}\u{0008}\n");
    }

    #[test]
    fn tty_backspace_ascii_bs_alias_matches_del() {
        let (tty, stdin_buf, stdout_buf) = new_tty(true, true);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("ab".to_string()),
                InputEvent::Data("\u{0008}".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect BS (0x08) to behave like DEL (0x7F) for canonical erase.
        assert_eq!(captured(&stdin_buf), "a\n");
        assert_eq!(
            captured(&stdout_buf),
            format!("ab{}\n", "\u{0008} \u{0008}")
        );
    }

    #[test]
    fn tty_mode_switch_clears_pending_parser_state() {
        let (mut tty, stdin_buf, _) = new_tty(false, true);
        tty = run_event(tty, InputEvent::Data("\u{001B}".to_string()));
        tty.options().set_line_buffering(false);
        tty = run_event(tty, InputEvent::Data("raw".to_string()));
        tty.options().set_line_buffering(true);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("x".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect parser state from canonical mode not to leak across raw-mode toggles.
        assert_eq!(captured(&stdin_buf), "rawx\n");
    }

    #[test]
    fn tty_ctrl_backslash_signals_sigquit_and_clears_line() {
        let (mut tty, stdin_buf, stdout_buf) = new_tty(true, true);
        let signals = Arc::new(Mutex::new(Vec::new()));
        tty.set_signaler(Box::new(RecordingSignaler::new(signals.clone())));

        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("abc".to_string()),
                InputEvent::Data("\u{001C}".to_string()),
                InputEvent::Data("x".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect ctrl-\ to signal SIGQUIT and clear current line before continuing.
        assert_eq!(captured(&stdin_buf), "x\n");
        assert_eq!(captured(&stdout_buf), "abc\nx\n");
        assert_eq!(signals.lock().unwrap().as_slice(), &[Signal::Sigquit as u8]);
    }

    #[test]
    fn tty_ctrl_z_signals_sigtstp_and_clears_line() {
        let (mut tty, stdin_buf, stdout_buf) = new_tty(true, true);
        let signals = Arc::new(Mutex::new(Vec::new()));
        tty.set_signaler(Box::new(RecordingSignaler::new(signals.clone())));

        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("abc".to_string()),
                InputEvent::Data("\u{001A}".to_string()),
                InputEvent::Data("x".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect ctrl-z to signal SIGTSTP and clear current line before continuing.
        assert_eq!(captured(&stdin_buf), "x\n");
        assert_eq!(captured(&stdout_buf), "abc\nx\n");
        assert_eq!(signals.lock().unwrap().as_slice(), &[Signal::Sigtstp as u8]);
    }

    #[test]
    fn tty_ignore_cr_option_ignores_carriage_return() {
        let (tty, stdin_buf, _) = new_tty(false, true);
        tty.options().set_ignore_cr(true);
        tty.options().set_map_cr_to_lf(false);

        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("abc".to_string()),
                InputEvent::Data("\r".to_string()),
                InputEvent::Data("x".to_string()),
                InputEvent::Data("\n".to_string()),
            ],
        );

        // Expect CR to be ignored and LF to flush the full buffered line.
        assert_eq!(captured(&stdin_buf), "abcx\n");
    }

    #[test]
    fn tty_disable_cr_to_lf_mapping_treats_cr_as_data() {
        let (tty, stdin_buf, _) = new_tty(false, true);
        tty.options().set_map_cr_to_lf(false);

        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("abc".to_string()),
                InputEvent::Data("\r".to_string()),
                InputEvent::Data("\n".to_string()),
            ],
        );

        // Expect CR to be buffered as data when CR->LF mapping is disabled.
        assert_eq!(captured(&stdin_buf), "abc\r\n");
    }

    #[test]
    fn tty_left_arrow_inline_insert_repaints_tail_exactly() {
        let (tty, _, stdout_buf) = new_tty(true, true);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("ab".to_string()),
                InputEvent::Data("\u{001B}\u{005B}\u{0044}".to_string()),
                InputEvent::Data("X".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect tail redraw after inline insert: X + shifted tail + cursor restore.
        assert_eq!(captured(&stdout_buf), "abXb\u{0008}\n");
    }

    #[test]
    fn tty_application_cursor_left_sequence_moves_cursor() {
        let (tty, stdin_buf, _) = new_tty(false, true);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("ab".to_string()),
                InputEvent::Data("\u{001B}OD".to_string()),
                InputEvent::Data("X".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect cursor-app-mode left sequence to be treated as cursor-left.
        assert_eq!(captured(&stdin_buf), "aXb\n");
    }

    #[test]
    fn tty_home_end_tilde_variants_are_consumed() {
        let (tty, stdin_buf, stdout_buf) = new_tty(true, true);
        let _tty = run_events(
            tty,
            vec![
                InputEvent::Data("\u{001B}[1~".to_string()),
                InputEvent::Data("\u{001B}[4~".to_string()),
                InputEvent::Data("x".to_string()),
                InputEvent::Data("\r".to_string()),
            ],
        );

        // Expect common Home/End tilde variants to be consumed as navigation keys.
        assert_eq!(captured(&stdin_buf), "x\n");
        assert_eq!(captured(&stdout_buf), "x\n");
    }

    #[test]
    fn tty_state_default_size_matches_console_defaults() {
        let tty_state = WasiTtyState::default();

        // Expect terminal defaults to match the conventional 80x25 console geometry.
        assert_eq!(tty_state.cols, 80);
        assert_eq!(tty_state.rows, 25);
    }
}
