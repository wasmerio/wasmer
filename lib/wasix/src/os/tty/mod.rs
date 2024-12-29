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
                        if let Some((what, when)) = self.last.as_ref() {
                            if what.as_str() == data && now - *when < TTY_MOBILE_PAUSE {
                                self.last = None;
                                return self;
                            }
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
