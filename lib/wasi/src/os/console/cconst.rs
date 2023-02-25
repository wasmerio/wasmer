#![allow(dead_code)]
pub struct ConsoleConst {}

impl ConsoleConst {
    pub const TERM_KEY_ENTER: u32 = 13;
    pub const TERM_KEY_BACKSPACE: u32 = 8;
    pub const TERM_KEY_INSERT: u32 = 45;
    pub const TERM_KEY_DEL: u32 = 46;
    pub const TERM_KEY_TAB: u32 = 9;
    pub const TERM_KEY_HOME: u32 = 36;
    pub const TERM_KEY_END: u32 = 35;
    pub const TERM_KEY_PAGE_UP: u32 = 33;
    pub const TERM_KEY_PAGE_DOWN: u32 = 34;
    pub const TERM_KEY_LEFT_ARROW: u32 = 37;
    pub const TERM_KEY_UP_ARROW: u32 = 38;
    pub const TERM_KEY_RIGHT_ARROW: u32 = 39;
    pub const TERM_KEY_DOWN_ARROW: u32 = 40;
    pub const TERM_KEY_C: u32 = 67;
    pub const TERM_KEY_L: u32 = 76;
    pub const TERM_KEY_F1: u32 = 112;
    pub const TERM_KEY_F2: u32 = 113;
    pub const TERM_KEY_F3: u32 = 114;
    pub const TERM_KEY_F4: u32 = 115;
    pub const TERM_KEY_F5: u32 = 116;
    pub const TERM_KEY_F6: u32 = 117;
    pub const TERM_KEY_F7: u32 = 118;
    pub const TERM_KEY_F8: u32 = 119;
    pub const TERM_KEY_F9: u32 = 120;
    pub const TERM_KEY_F10: u32 = 121;
    pub const TERM_KEY_F11: u32 = 122;
    pub const TERM_KEY_F12: u32 = 123;

    pub const TERM_CURSOR_UP: &'static str = "\x1b[A";
    pub const TERM_CURSOR_DOWN: &'static str = "\x1b[B";
    pub const TERM_CURSOR_RIGHT: &'static str = "\x1b[C";
    pub const TERM_CURSOR_LEFT: &'static str = "\x1b[D";

    pub const TERM_DELETE_LINE: &'static str = "\x1b[2K\r";
    pub const TERM_DELETE_RIGHT: &'static str = "\x1b[0K\r";
    pub const TERM_DELETE_LEFT: &'static str = "\x1b[1K\r";
    pub const TERM_DELETE_BELOW: &'static str = "\x1b[0J\r";
    pub const TERM_DELETE_ABOVE: &'static str = "\x1b[1J\r";
    pub const TERM_DELETE_ALL: &'static str = "\x1b[2J\r";
    pub const TERM_DELETE_SAVED: &'static str = "\x1b[3J\r";

    pub const TERM_CURSOR_SAVE: &'static str = "\x1b[s";
    pub const TERM_CURSOR_RESTORE: &'static str = "\x1b[u";

    pub const TERM_WRAPAROUND: &'static str = "\x1b[?7h";
    pub const TERM_REVERSE_WRAPAROUND: &'static str = "\x1b[?45h";

    pub const TERM_NO_WRAPAROUND: &'static str = "\x1b[?7l";
    pub const TERM_NO_REVERSE_WRAPAROUND: &'static str = "\x1b[?45l";

    pub const COL_RESET: &'static str = "\x1B[0m";
    pub const COL_BLACK: &'static str = "\x1B[0;30m";
    pub const COL_GRAY: &'static str = "\x1B[1;30m";
    pub const COL_RED: &'static str = "\x1B[0;31m";
    pub const COL_LIGHT_RED: &'static str = "\x1B[1;31m";
    pub const COL_GREEN: &'static str = "\x1B[0;32m";
    pub const COL_LIGHT_GREEN: &'static str = "\x1B[1;32m";
    pub const COL_BROWN: &'static str = "\x1B[0;33m";
    pub const COL_YELLOW: &'static str = "\x1B[1;33m";
    pub const COL_BLUE: &'static str = "\x1B[0;34m";
    pub const COL_LIGHT_BLUE: &'static str = "\x1B[1;34m";
    pub const COL_PURPLE: &'static str = "\x1B[0;35m";
    pub const COL_LIGHT_PURPLE: &'static str = "\x1B[1;35m";
    pub const COL_CYAN: &'static str = "\x1B[0;36m";
    pub const COL_LIGHT_CYAN: &'static str = "\x1B[1;36m";
    pub const COL_LIGHT_GRAY: &'static str = "\x1B[0;37m";
    pub const COL_WHITE: &'static str = "\x1B[1;37m";

    pub const WELCOME_LARGE: &'static str = include_str!("txt/welcome_large.txt");
    pub const WELCOME_MEDIUM: &'static str = include_str!("txt/welcome_medium.txt");
    pub const WELCOME_SMALL: &'static str = include_str!("txt/welcome_small.txt");

    pub const ABOUT: &'static str = include_str!("txt/about.md");
    pub const ABOUT_WASMER: &'static str = include_str!("txt/about_wasmer.md");
    pub const HELP: &'static str = include_str!("txt/help.md");
    pub const BAD_WORKER: &'static str = include_str!("txt/bad_worker.md");
}
