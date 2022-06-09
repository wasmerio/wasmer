// input encoding

pub const KEY_PRESS: u8 = 1;
pub const MOUSE_MOVE: u8 = 2;
pub const KEY_RELEASE: u8 = 3;
pub const MOUSE_PRESS_LEFT: u8 = 4;
pub const MOUSE_PRESS_RIGHT: u8 = 5;
pub const MOUSE_PRESS_MIDDLE: u8 = 7;
pub const WINDOW_CLOSED: u8 = 8;

use minifb::{Key, MouseButton};

#[derive(Debug, Clone, Copy)]
pub enum InputEvent {
    KeyPress(Key),
    KeyRelease(Key),
    MouseEvent(u32, u32, MouseButton),
    MouseMoved(u32, u32),
    WindowClosed,
}

/// Returns the tag as the first return value
/// The data as the second return value
/// and the amount of data to read from it as the third value
pub fn bytes_for_input_event(input_event: InputEvent) -> (u8, [u8; 8], usize) {
    let mut data = [0u8; 8];
    match input_event {
        InputEvent::KeyPress(k) => {
            data[0] = map_key_to_bytes(k);
            (KEY_PRESS, data, 1)
        }
        InputEvent::KeyRelease(k) => {
            data[0] = map_key_to_bytes(k);
            (KEY_RELEASE, data, 1)
        }
        InputEvent::MouseEvent(x, y, btn) => {
            let tag = match btn {
                MouseButton::Left => MOUSE_PRESS_LEFT,
                MouseButton::Right => MOUSE_PRESS_RIGHT,
                MouseButton::Middle => MOUSE_PRESS_MIDDLE,
            };
            let x_bytes = x.to_le_bytes();
            data[..4].clone_from_slice(&x_bytes[..4]);
            let y_bytes = y.to_le_bytes();
            data[4..8].clone_from_slice(&y_bytes[..4]);
            (tag, data, 8)
        }
        InputEvent::MouseMoved(x, y) => {
            let x_bytes = x.to_le_bytes();
            data[..4].clone_from_slice(&x_bytes[..4]);
            let y_bytes = y.to_le_bytes();
            data[4..8].clone_from_slice(&y_bytes[..4]);
            (MOUSE_MOVE, data, 8)
        }
        InputEvent::WindowClosed => (WINDOW_CLOSED, data, 0),
    }
}

pub fn map_key_to_bytes(key: Key) -> u8 {
    match key {
        Key::Backspace => 8,
        Key::Tab => 9,
        Key::NumPadEnter | Key::Enter => 13,
        Key::LeftShift | Key::RightShift => 16,
        Key::LeftCtrl | Key::RightCtrl => 17,
        Key::LeftAlt | Key::RightAlt => 18,
        Key::Pause => 19,
        Key::CapsLock => 20,
        Key::Escape => 27,
        Key::Space => 32,
        Key::PageUp => 33,
        Key::PageDown => 34,
        Key::End => 35,
        Key::Home => 36,

        Key::Left => 37,
        Key::Up => 38,
        Key::Right => 39,
        Key::Down => 40,

        Key::Insert => 45,
        Key::Delete => 46,

        Key::Key0 => 48,
        Key::Key1 => 49,
        Key::Key2 => 50,
        Key::Key3 => 51,
        Key::Key4 => 52,
        Key::Key5 => 53,
        Key::Key6 => 54,
        Key::Key7 => 55,
        Key::Key8 => 56,
        Key::Key9 => 57,

        Key::A => b'A',
        Key::B => b'B',
        Key::C => b'C',
        Key::D => b'D',
        Key::E => b'E',
        Key::F => b'F',
        Key::G => b'G',
        Key::H => b'H',
        Key::I => b'I',
        Key::J => b'J',
        Key::K => b'K',
        Key::L => b'L',
        Key::M => b'M',
        Key::N => b'N',
        Key::O => b'O',
        Key::P => b'P',
        Key::Q => b'Q',
        Key::R => b'R',
        Key::S => b'S',
        Key::T => b'T',
        Key::U => b'U',
        Key::V => b'V',
        Key::W => b'W',
        Key::X => b'X',
        Key::Y => b'Y',
        Key::Z => b'Z',

        Key::LeftSuper => 91,
        Key::RightSuper => 92,

        Key::NumPad0 => 96,
        Key::NumPad1 => 97,
        Key::NumPad2 => 98,
        Key::NumPad3 => 99,
        Key::NumPad4 => 100,
        Key::NumPad5 => 101,
        Key::NumPad6 => 102,
        Key::NumPad7 => 103,
        Key::NumPad8 => 104,
        Key::NumPad9 => 105,
        Key::NumPadAsterisk => 106,
        Key::NumPadPlus => 107,
        Key::NumPadMinus => 109,
        Key::NumPadDot => 110,
        Key::NumPadSlash => 111,

        Key::F1 => 112,
        Key::F2 => 113,
        Key::F3 => 114,
        Key::F4 => 115,
        Key::F5 => 116,
        Key::F6 => 117,
        Key::F7 => 118,
        Key::F8 => 119,
        Key::F9 => 120,
        Key::F10 => 121,
        Key::F11 => 122,
        Key::F12 => 123,

        Key::NumLock => 144,
        Key::ScrollLock => 145,

        Key::Semicolon => 186,
        Key::Equal => 187,
        Key::Comma => 188,
        Key::Minus => 189,
        Key::Period => 190,
        Key::Slash => 191,
        Key::Backquote => 192,
        Key::Backslash => 220,
        Key::Apostrophe => 220,

        Key::LeftBracket => 219,
        Key::RightBracket => 221,

        _ => 255,
    }
}
