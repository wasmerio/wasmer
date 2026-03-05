use std::{
    borrow::Cow,
    sync::{Arc, Mutex},
};

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
    line_feeds: bool,
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
}

#[derive(Debug)]
pub struct Tty {
    stdin: Box<dyn VirtualFile + Send + Sync + 'static>,
    stdout: Box<dyn VirtualFile + Send + Sync + 'static>,
    signaler: Option<Box<dyn SignalHandlerAbi + Send + Sync + 'static>>,
    is_mobile: bool,
    last: Option<(String, u128)>,
    options: TtyOptions,
    line: String,
}

impl Tty {
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
            line: String::new(),
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

                    self.on_data(data.as_bytes().to_vec().into()).await
                }
                InputEvent::Raw(data) => self.on_data(data.into()).await,
            }
        })
    }

    fn on_enter(mut self, _data: Cow<'static, [u8]>) -> BoxFuture<'static, Self> {
        Box::pin(async move {
            // Add a line feed on the end and take the line
            let mut data = self.line.clone();
            self.line.clear();
            data.push('\n');

            // If echo is on then write a new line
            {
                let echo = {
                    let options = self.options.inner.lock().unwrap();
                    options.echo
                };
                if echo {
                    let _ = self.stdout.write("\n".as_bytes()).await;
                }
            }

            // Send the data to the process
            let _ = self.stdin.write(data.as_bytes()).await;
            self
        })
    }

    fn on_ctrl_c(mut self, _data: Cow<'static, [u8]>) -> BoxFuture<'static, Self> {
        Box::pin(async move {
            if let Some(signaler) = self.signaler.as_ref() {
                signaler.signal(Signal::Sigint as u8).ok();

                let (echo, _line_buffering) = {
                    let options = self.options.inner.lock().unwrap();
                    (options.echo, options.line_buffering)
                };

                self.line.clear();
                if echo {
                    let _ = self.stdout.write("\n".as_bytes()).await;
                }
            }
            self
        })
    }

    fn on_backspace(mut self, _data: Cow<'static, [u8]>) -> BoxFuture<'static, Self> {
        // Remove a character (if there are none left we are done)
        if self.line.is_empty() {
            return Box::pin(async move { self });
        }
        let len = self.line.len();
        self.line = self.line[..len - 1].to_string();

        Box::pin(async move {
            // If echo is on then write the backspace
            {
                let echo = {
                    let options = self.options.inner.lock().unwrap();
                    options.echo
                };
                if echo {
                    let _ = self.stdout.write("\u{0008} \u{0008}".as_bytes()).await;
                }
            }
            self
        })
    }

    fn on_tab(self, _data: Cow<'static, [u8]>) -> BoxFuture<'static, Self> {
        Box::pin(async move { self })
    }

    fn on_cursor_left(self, _data: Cow<'static, [u8]>) -> BoxFuture<'static, Self> {
        Box::pin(async move { self })
    }

    fn on_cursor_right(self, _data: Cow<'static, [u8]>) -> BoxFuture<'static, Self> {
        Box::pin(async move { self })
    }

    fn on_cursor_up(self, _data: Cow<'static, [u8]>) -> BoxFuture<'static, Self> {
        Box::pin(async move { self })
    }

    fn on_cursor_down(self, _data: Cow<'static, [u8]>) -> BoxFuture<'static, Self> {
        Box::pin(async move { self })
    }

    fn on_home(self, _data: Cow<'static, [u8]>) -> BoxFuture<'static, Self> {
        Box::pin(async move { self })
    }

    fn on_end(self, _data: Cow<'static, [u8]>) -> BoxFuture<'static, Self> {
        Box::pin(async move { self })
    }

    fn on_ctrl_l(self, _data: Cow<'static, [u8]>) -> BoxFuture<'static, Self> {
        Box::pin(async move { self })
    }

    fn on_page_up(self, _data: Cow<'static, [u8]>) -> BoxFuture<'static, Self> {
        Box::pin(async move { self })
    }

    fn on_page_down(self, _data: Cow<'static, [u8]>) -> BoxFuture<'static, Self> {
        Box::pin(async move { self })
    }

    fn on_f1(self, _data: Cow<'static, [u8]>) -> BoxFuture<'static, Self> {
        Box::pin(async move { self })
    }

    fn on_f2(self, _data: Cow<'static, [u8]>) -> BoxFuture<'static, Self> {
        Box::pin(async move { self })
    }

    fn on_f3(self, _data: Cow<'static, [u8]>) -> BoxFuture<'static, Self> {
        Box::pin(async move { self })
    }

    fn on_f4(self, _data: Cow<'static, [u8]>) -> BoxFuture<'static, Self> {
        Box::pin(async move { self })
    }

    fn on_f5(self, _data: Cow<'static, [u8]>) -> BoxFuture<'static, Self> {
        Box::pin(async move { self })
    }

    fn on_f6(self, _data: Cow<'static, [u8]>) -> BoxFuture<'static, Self> {
        Box::pin(async move { self })
    }

    fn on_f7(self, _data: Cow<'static, [u8]>) -> BoxFuture<'static, Self> {
        Box::pin(async move { self })
    }

    fn on_f8(self, _data: Cow<'static, [u8]>) -> BoxFuture<'static, Self> {
        Box::pin(async move { self })
    }

    fn on_f9(self, _data: Cow<'static, [u8]>) -> BoxFuture<'static, Self> {
        Box::pin(async move { self })
    }

    fn on_f10(self, _data: Cow<'static, [u8]>) -> BoxFuture<'static, Self> {
        Box::pin(async move { self })
    }

    fn on_f11(self, _data: Cow<'static, [u8]>) -> BoxFuture<'static, Self> {
        Box::pin(async move { self })
    }

    fn on_f12(self, _data: Cow<'static, [u8]>) -> BoxFuture<'static, Self> {
        Box::pin(async move { self })
    }

    fn on_data(mut self, data: Cow<'static, [u8]>) -> BoxFuture<'static, Self> {
        // If we are line buffering then we need to check for some special cases
        let options = { self.options.inner.lock().unwrap().clone() };
        if options.line_buffering {
            let echo = options.echo;
            return match String::from_utf8_lossy(data.as_ref()).as_ref() {
                "\r" | "\u{000A}" => self.on_enter(data),
                "\u{0003}" => self.on_ctrl_c(data),
                "\u{007F}" => self.on_backspace(data),
                "\u{0009}" => self.on_tab(data),
                "\u{001B}\u{005B}\u{0044}" => self.on_cursor_left(data),
                "\u{001B}\u{005B}\u{0043}" => self.on_cursor_right(data),
                "\u{0001}" | "\u{001B}\u{005B}\u{0048}" => self.on_home(data),
                "\u{001B}\u{005B}\u{0046}" => self.on_end(data),
                "\u{001B}\u{005B}\u{0041}" => self.on_cursor_up(data),
                "\u{001B}\u{005B}\u{0042}" => self.on_cursor_down(data),
                "\u{000C}" => self.on_ctrl_l(data),
                "\u{001B}\u{005B}\u{0035}\u{007E}" => self.on_page_up(data),
                "\u{001B}\u{005B}\u{0036}\u{007E}" => self.on_page_down(data),
                "\u{001B}\u{004F}\u{0050}" => self.on_f1(data),
                "\u{001B}\u{004F}\u{0051}" => self.on_f2(data),
                "\u{001B}\u{004F}\u{0052}" => self.on_f3(data),
                "\u{001B}\u{004F}\u{0053}" => self.on_f4(data),
                "\u{001B}\u{005B}\u{0031}\u{0035}\u{007E}" => self.on_f5(data),
                "\u{001B}\u{005B}\u{0031}\u{0037}\u{007E}" => self.on_f6(data),
                "\u{001B}\u{005B}\u{0031}\u{0038}\u{007E}" => self.on_f7(data),
                "\u{001B}\u{005B}\u{0031}\u{0039}\u{007E}" => self.on_f8(data),
                "\u{001B}\u{005B}\u{0032}\u{0030}\u{007E}" => self.on_f9(data),
                "\u{001B}\u{005B}\u{0032}\u{0031}\u{007E}" => self.on_f10(data),
                "\u{001B}\u{005B}\u{0032}\u{0033}\u{007E}" => self.on_f11(data),
                "\u{001B}\u{005B}\u{0032}\u{0034}\u{007E}" => self.on_f12(data),
                _ => Box::pin(async move {
                    if echo {
                        let _ = self.stdout.write(data.as_ref()).await;
                    }
                    self.line
                        .push_str(String::from_utf8_lossy(data.as_ref()).as_ref());
                    self
                }),
            };
        };

        Box::pin(async move {
            // If the echo is enabled then write it to the terminal
            if options.echo {
                // TODO: log / propagate error?
                let _ = self.stdout.write(data.as_ref()).await;
            }

            // Now send it to the process
            let _ = self.stdin.write(data.as_ref()).await;
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
            rows: 80,
            cols: 25,
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

    use super::{InputEvent, Tty, TtyOptions};
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
    fn tty_ctrl_c_without_signaler_is_noop_current_behavior() {
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

        // Expect current behavior: without a signal handler, ctrl-c is treated as ordinary input.
        assert_eq!(captured(&stdin_buf), "abcx\n");
        assert_eq!(captured(&stdout_buf), "abcx\n");
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
}
