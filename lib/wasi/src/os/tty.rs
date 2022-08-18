use std::{sync::{Mutex, Arc}, io::Write};
use derivative::*;

use wasmer_vfs::VirtualFile;
use wasmer_vbus::SignalHandlerAbi;
use wasmer_wasi_types::__WASI_CLOCK_MONOTONIC;

use crate::{
    types::__WASI_SIGINT,
    syscalls::platform_clock_time_get
};

const TTY_MOBILE_PAUSE: u128 = std::time::Duration::from_millis(200).as_nanos();

#[derive(Debug)]
pub enum InputEvent {
    Key,
    Data(String),
    Raw(Vec<u8>),
}

#[derive(Debug)]
pub struct ConsoleRect {
    pub cols: u32,
    pub rows: u32,
}

impl Default
for ConsoleRect {
    fn default() -> Self {
        Self {
            cols: 80,
            rows: 25
        }
    }
}

#[derive(Debug)]
pub struct TtyOptionsInner {
    echo: bool,
    line_buffering: bool,
    line_feeds: bool,
    rect: ConsoleRect,
}

#[derive(Debug, Clone)]
pub struct TtyOptions {
    inner: Arc<Mutex<TtyOptionsInner>>
}

impl Default
for TtyOptions {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(TtyOptionsInner {
                echo: true,
                line_buffering: true,
                line_feeds: true,
                rect: ConsoleRect {
                    cols: 80,
                    rows: 25
                }
            }))
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

#[derive(Derivative)]
#[derivative(Debug)]
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
        options: TtyOptions
    ) -> Self {
        Self {
            stdin,
            stdout,
            signaler: None,
            last: None,
            options,
            is_mobile,
            line: String::new()
        }
    }

    pub fn options(&self) -> TtyOptions {
        self.options.clone()
    }

    pub fn set_signaler(&mut self, signaler: Box<dyn SignalHandlerAbi + Send + Sync + 'static>) {
        self.signaler.replace(signaler);
    }

    pub fn on_event(&mut self, event: InputEvent) {
        match event {
            InputEvent::Key => {
                // do nothing
            }
            InputEvent::Data(data) => {
                // Due to a nasty bug in xterm.js on Android mobile it sends the keys you press
                // twice in a row with a short interval between - this hack will avoid that bug
                if self.is_mobile {
                    let now = platform_clock_time_get(__WASI_CLOCK_MONOTONIC, 1_000_000).unwrap() as u128;
                    if let Some((what, when)) = self.last.as_ref() {
                        if what.as_str() == data && now - *when < TTY_MOBILE_PAUSE {
                            self.last = None;
                            return;
                        }
                    }
                    self.last = Some((data.clone(), now))
                }

                self.on_data(data.as_bytes())
            }
            InputEvent::Raw(data) => {
                self.on_data(&data[..])
            }
        }
    }

    fn on_enter(&mut self, _data: &str)
    {
        // Add a line feed on the end and take the line
        let mut data = self.line.clone();
        self.line.clear();
        data.push_str("\n");

        // If echo is on then write a new line
        {
            let options = self.options.inner.lock().unwrap();
            if options.echo {
                drop(options);
                self.stdout("\n".as_bytes());
            }
        }

        // Send the data to the process
        let _ = self.stdin.write(data.as_bytes());
    }

    fn on_ctrl_c(&mut self, _data: &str)
    {
        if let Some(signaler) = self.signaler.as_ref() {
            signaler.signal(__WASI_SIGINT);

            let (echo, _line_buffering) = {
                let options = self.options.inner.lock().unwrap();
                (options.echo, options.line_buffering)
            };
    
            self.line.clear();
            if echo {
                self.stdout("\n".as_bytes());
            }
            let _ = self.stdin.write("\n".as_bytes());
        }
    }

    fn on_backspace(&mut self, _data: &str)
    {
        // Remove a character (if there are none left we are done)
        if self.line.is_empty() {
            return;
        }
        let len = self.line.len();
        self.line = (&self.line[..len-1]).to_string();
        
        // If echo is on then write the backspace
        {
            let options = self.options.inner.lock().unwrap();
            if options.echo {
                drop(options);
                self.stdout("\u{0008} \u{0008}".as_bytes());
            }
        }
    }

    fn on_tab(&mut self, _data: &str)
    {
    }

    fn on_cursor_left(&mut self, _data: &str)
    {
    }

    fn on_cursor_right(&mut self, _data: &str)
    {
    }

    fn on_cursor_up(&mut self, _data: &str)
    {
    }

    fn on_cursor_down(&mut self, _data: &str)
    {
    }

    fn on_home(&mut self, _data: &str)
    {
    }

    fn on_end(&mut self, _data: &str)
    {
    }

    fn on_ctrl_l(&mut self, _data: &str)
    {
    }

    fn on_page_up(&mut self, _data: &str)
    {
    }

    fn on_page_down(&mut self, _data: &str)
    {
    }

    fn on_f1(&mut self, _data: &str)
    {
    }

    fn on_f2(&mut self, _data: &str)
    {
    }

    fn on_f3(&mut self, _data: &str)
    {
    }

    fn on_f4(&mut self, _data: &str)
    {
    }

    fn on_f5(&mut self, _data: &str)
    {
    }

    fn on_f6(&mut self, _data: &str)
    {
    }

    fn on_f7(&mut self, _data: &str)
    {
    }

    fn on_f8(&mut self, _data: &str)
    {
    }

    fn on_f9(&mut self, _data: &str)
    {
    }

    fn on_f10(&mut self, _data: &str)
    {
    }

    fn on_f11(&mut self, _data: &str)
    {
    }

    fn on_f12(&mut self, _data: &str)
    {
    }

    fn on_data(&mut self, data: &[u8])
    {
        // If we are line buffering then we need to check for some special cases
        let options = self.options.inner.lock().unwrap();
        if options.line_buffering {
            let echo = options.echo;
            drop(options);
            let data = String::from_utf8_lossy(data);
            let data = data.as_ref();
            return match data {
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
                data => {
                    if echo == true {
                        self.stdout(data.as_bytes());
                    }
                    self.line.push_str(data);
                }
            };
        };

        // If the echo is enabled then write it to the terminal
        if options.echo == true {
            drop(options);
            self.stdout(data);
        } else {
            drop(options);
        }

        // Now send it to the process
        let _ = self.stdin.write(data);
    }

    fn stdout(&mut self, data: &[u8]) {
        let _ = self.stdout.write(&data[..]);
    }
}