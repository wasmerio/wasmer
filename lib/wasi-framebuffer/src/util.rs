// input encoding

pub const KEY_PRESS: u8 = 1;
pub const MOUSE_MOVE: u8 = 2;
pub const MOUSE_PRESS_LEFT: u8 = 4;
pub const MOUSE_PRESS_RIGHT: u8 = 5;
pub const MOUSE_PRESS_MIDDLE: u8 = 7;

use minifb::{Key, MouseButton};

#[derive(Debug, Clone, Copy)]
pub enum InputEvent {
    KeyPress(Key),
    MouseEvent(u32, u32, MouseButton),
    MouseMoved(u32, u32),
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
        InputEvent::MouseEvent(x, y, btn) => {
            let tag = match btn {
                MouseButton::Left => MOUSE_PRESS_LEFT,
                MouseButton::Right => MOUSE_PRESS_RIGHT,
                MouseButton::Middle => MOUSE_PRESS_MIDDLE,
            };
            let x_bytes = x.to_le_bytes();
            for i in 0..4 {
                data[i] = x_bytes[i];
            }
            let y_bytes = y.to_le_bytes();
            for i in 0..4 {
                data[i + 4] = y_bytes[i];
            }
            (tag, data, 8)
        }
        InputEvent::MouseMoved(x, y) => {
            let x_bytes = x.to_le_bytes();
            for i in 0..4 {
                data[i] = x_bytes[i];
            }
            let y_bytes = y.to_le_bytes();
            for i in 0..4 {
                data[i + 4] = y_bytes[i];
            }
            (MOUSE_MOVE, data, 8)
        }
    }
}

pub fn map_key_to_bytes(key: Key) -> u8 {
    match key {
        Key::Key0 => b'0',
        Key::Key1 => b'1',
        Key::Key2 => b'2',
        Key::Key3 => b'3',
        Key::Key4 => b'4',
        Key::Key5 => b'5',
        Key::Key6 => b'6',
        Key::Key7 => b'7',
        Key::Key8 => b'8',
        Key::Key9 => b'9',
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
        Key::F1 => 131,
        Key::F2 => 132,
        Key::F3 => 133,
        Key::F4 => 134,
        Key::F5 => 135,
        Key::F6 => 136,
        Key::F7 => 137,
        Key::F8 => 138,
        Key::F9 => 139,
        Key::F10 => 140,
        Key::F11 => 141,
        Key::F12 => 142,
        Key::F13 => 143,
        Key::F14 => 144,
        Key::F15 => 145,

        Key::Down => 146,
        Key::Left => 147,
        Key::Right => 148,
        Key::Up => 149,
        Key::Apostrophe => b'\'',
        Key::Backquote => b'`',

        Key::Backslash => b'\\',
        Key::Comma => b',',
        Key::Equal => b'=',
        Key::LeftBracket => b'[',
        Key::Minus => b'-',
        Key::Period => b'.',
        Key::RightBracket => b']',
        Key::Semicolon => b';',

        Key::Slash => b'/',
        Key::Backspace => 128,
        Key::Delete => 127,
        Key::End => 150,
        Key::Enter => b'\n',

        Key::Escape => 28,

        Key::Home => 151,
        Key::Insert => 152,
        Key::Menu => 153,

        Key::PageDown => 154,
        Key::PageUp => 155,

        Key::Pause => 156,
        Key::Space => b' ',
        Key::Tab => b'\t',
        Key::NumLock => 157,
        Key::CapsLock => 158,
        Key::ScrollLock => 159,
        Key::LeftShift => 160,
        Key::RightShift => 161,
        Key::LeftCtrl => 162,
        Key::RightCtrl => 163,

        Key::NumPad0 => 170,
        Key::NumPad1 => 171,
        Key::NumPad2 => 172,
        Key::NumPad3 => 173,
        Key::NumPad4 => 174,
        Key::NumPad5 => 175,
        Key::NumPad6 => 176,
        Key::NumPad7 => 177,
        Key::NumPad8 => 178,
        Key::NumPad9 => 179,
        Key::NumPadDot => 180,
        Key::NumPadSlash => 181,
        Key::NumPadAsterisk => 182,
        Key::NumPadMinus => 183,
        Key::NumPadPlus => 184,
        Key::NumPadEnter => 185,

        Key::LeftAlt => 186,
        Key::RightAlt => 187,

        Key::LeftSuper => 188,
        Key::RightSuper => 189,

        _ => 255,
    }
}
