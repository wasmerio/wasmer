#![cfg(feature = "link_external_libs")]

use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, VecDeque};
use std::convert::TryInto;
use std::io::{Read, Seek, SeekFrom, Write};
use tracing::debug;
use wasmer_wasi::{
    types::{wasi::Filesize, *},
    WasiInodes,
};
use wasmer_wasi::{Fd, VirtualFile, WasiFs, WasiFsError, ALL_RIGHTS, VIRTUAL_ROOT_FD};

use minifb::{Key, KeyRepeat, MouseButton, Scale, Window, WindowOptions};

mod util;

use util::*;

use std::cell::RefCell;
std::thread_local! {
    pub(crate) static FRAMEBUFFER_STATE: RefCell<FrameBufferState> =
        RefCell::new(FrameBufferState::new()
);
}

pub const MAX_X: u32 = 8192;
pub const MAX_Y: u32 = 4320;

#[derive(Debug, Serialize, Deserialize)]
pub enum FrameBufferFileType {
    Buffer,
    Resolution,
    Draw,
    Input,
}

#[derive(Debug)]
pub(crate) struct FrameBufferState {
    // double buffered
    pub data_1: Vec<u32>,
    pub data_2: Vec<u32>,

    pub x_size: u32,
    pub y_size: u32,
    pub front_buffer: bool,

    pub window: Window,

    pub last_mouse_pos: (u32, u32),
    pub inputs: VecDeque<InputEvent>,
    pub keys_pressed: BTreeSet<minifb::Key>,
}

impl FrameBufferState {
    /// an arbitrary large number
    const MAX_INPUTS: usize = 128;

    pub fn new() -> Self {
        let x = 100;
        let y = 200;

        let window = Self::create_window(x, y);

        Self {
            data_1: vec![0; x * y],
            data_2: vec![0; x * y],

            x_size: x as u32,
            y_size: y as u32,
            front_buffer: true,

            window,
            last_mouse_pos: (0, 0),
            inputs: VecDeque::with_capacity(Self::MAX_INPUTS),
            keys_pressed: BTreeSet::new(),
        }
    }

    fn create_window(x: usize, y: usize) -> Window {
        Window::new(
            "Wasmer Experimental FrameBuffer",
            x,
            y,
            WindowOptions {
                resize: true,
                scale: Scale::FitScreen,
                ..WindowOptions::default()
            },
        )
        .unwrap()
    }

    pub fn resize(&mut self, x: u32, y: u32) -> Option<()> {
        if x >= MAX_X || y >= MAX_Y {
            return None;
        }
        self.x_size = x;
        self.y_size = x;

        self.data_1.resize((x * y) as usize, 0);
        self.data_2.resize((x * y) as usize, 0);

        self.window = Self::create_window(x as usize, y as usize);

        Some(())
    }

    fn push_input_event(&mut self, input_event: InputEvent) -> Option<()> {
        if self.inputs.len() >= Self::MAX_INPUTS {
            return None;
        }

        self.inputs.push_back(input_event);
        Some(())
    }

    pub fn fill_input_buffer(&mut self) -> Option<()> {
        let keys_pressed = self.keys_pressed.iter().cloned().collect::<Vec<Key>>();
        if !self.window.is_open() {
            self.push_input_event(InputEvent::WindowClosed)?;
        }
        for key in keys_pressed {
            if self.window.is_key_released(key) {
                self.keys_pressed.remove(&key);
                self.push_input_event(InputEvent::KeyRelease(key))?;
            }
        }
        let keys = self.window.get_keys_pressed(KeyRepeat::No)?;
        for key in keys {
            self.keys_pressed.insert(key);
            self.push_input_event(InputEvent::KeyPress(key))?;
        }

        let mouse_position = self.window.get_mouse_pos(minifb::MouseMode::Clamp)?;
        if mouse_position.0 as u32 != self.last_mouse_pos.0
            || mouse_position.1 as u32 != self.last_mouse_pos.1
        {
            self.last_mouse_pos = (mouse_position.0 as u32, mouse_position.1 as u32);
            self.push_input_event(InputEvent::MouseMoved(
                self.last_mouse_pos.0,
                self.last_mouse_pos.1,
            ))?;
        }

        if self.window.get_mouse_down(MouseButton::Left) {
            self.push_input_event(InputEvent::MouseEvent(
                mouse_position.0 as u32,
                mouse_position.1 as u32,
                MouseButton::Left,
            ))?;
        }
        if self.window.get_mouse_down(MouseButton::Right) {
            self.push_input_event(InputEvent::MouseEvent(
                mouse_position.0 as u32,
                mouse_position.1 as u32,
                MouseButton::Right,
            ))?;
        }
        if self.window.get_mouse_down(MouseButton::Middle) {
            self.push_input_event(InputEvent::MouseEvent(
                mouse_position.0 as u32,
                mouse_position.1 as u32,
                MouseButton::Middle,
            ))?;
        }
        Some(())
    }

    pub fn draw(&mut self) {
        self.window
            .update_with_buffer(
                if self.front_buffer {
                    &self.data_1[..]
                } else {
                    &self.data_2[..]
                },
                self.x_size.try_into().unwrap(),
                self.y_size.try_into().unwrap(),
            )
            .expect("Internal error! Failed to draw to framebuffer");
    }

    #[inline]
    // the real index into u32s and whether to use the front buffer or the back buffer
    fn get_idx_info(&self, idx: usize) -> Option<(usize, bool)> {
        let mut base_idx = idx / 4;
        let mut front_buffer = true;

        if base_idx >= self.data_1.len() {
            base_idx -= self.data_1.len();
            front_buffer = false;

            if base_idx >= self.data_2.len() {
                return None;
            }
        }

        Some((base_idx, front_buffer))
    }

    pub fn get_byte(&self, idx: usize) -> Option<u8> {
        let (base_idx, front_buffer) = self.get_idx_info(idx)?;

        let shift = idx % 4;
        let shift_amt = 8 * shift;

        if front_buffer {
            Some((self.data_1[base_idx] >> shift_amt) as u8)
        } else {
            Some((self.data_2[base_idx] >> shift_amt) as u8)
        }
    }

    pub fn set_byte(&mut self, idx: usize, val: u8) -> Option<()> {
        let (base_idx, front_buffer) = self.get_idx_info(idx)?;

        let shift = idx % 4;
        let shift_amt = 8 * shift;

        if front_buffer {
            self.data_1[base_idx] &= !(0xFF << shift_amt);
            self.data_1[base_idx] |= ((val as u32) << shift_amt) & (0xFF << shift_amt);
        } else {
            self.data_2[base_idx] &= !(0xFF << shift_amt);
            self.data_2[base_idx] |= ((val as u32) << shift_amt) & (0xFF << shift_amt);
        }
        Some(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FrameBuffer {
    fb_type: FrameBufferFileType,
    cursor: u32,
}

impl Read for FrameBuffer {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let cursor = self.cursor as usize;
        FRAMEBUFFER_STATE.with(|fb| {
            let mut fb_state = fb.borrow_mut();
            match self.fb_type {
                FrameBufferFileType::Buffer => {
                    let mut bytes_copied = 0;

                    for (i, b) in buf.iter_mut().enumerate() {
                        if let Some(byte) = fb_state.get_byte(cursor + i) {
                            *b = byte;
                            bytes_copied += 1;
                        } else {
                            break;
                        }
                    }

                    self.cursor += bytes_copied;
                    Ok(bytes_copied as usize)
                }
                FrameBufferFileType::Resolution => {
                    let resolution_data = format!("{}x{}", fb_state.x_size, fb_state.y_size);

                    let mut bytes = resolution_data.bytes().skip(cursor);
                    let bytes_to_copy = std::cmp::min(buf.len(), bytes.clone().count());

                    for byte in buf.iter_mut().take(bytes_to_copy) {
                        *byte = bytes.next().unwrap();
                    }

                    self.cursor += bytes_to_copy as u32;
                    Ok(bytes_to_copy)
                }

                FrameBufferFileType::Draw => {
                    if buf.is_empty() {
                        Ok(0)
                    } else {
                        buf[0] = fb_state.front_buffer as u8 + b'0';
                        Ok(1)
                    }
                }

                FrameBufferFileType::Input => {
                    let mut idx = 0;
                    fb_state.fill_input_buffer();

                    while let Some(next_elem) = fb_state.inputs.front() {
                        let remaining_length = buf.len() - idx;
                        let (tag_byte, data, size) = bytes_for_input_event(*next_elem);
                        if remaining_length > 1 + size {
                            buf[idx] = tag_byte;
                            for i in 0..size {
                                buf[idx + 1 + i] = data[i];
                            }
                            idx += 1 + size;
                        } else {
                            break;
                        }
                        fb_state.inputs.pop_front().unwrap();
                    }
                    Ok(idx)
                }
            }
        })
    }

    fn read_to_end(&mut self, _buf: &mut Vec<u8>) -> std::io::Result<usize> {
        unimplemented!()
    }
    fn read_to_string(&mut self, _buf: &mut String) -> std::io::Result<usize> {
        unimplemented!()
    }
    fn read_exact(&mut self, _buf: &mut [u8]) -> std::io::Result<()> {
        unimplemented!()
    }
}

impl Seek for FrameBuffer {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        match pos {
            SeekFrom::Current(offset) => {
                let result: std::io::Result<u64> = (self.cursor as i64)
                    .checked_add(offset)
                    .and_then(|v| v.try_into().ok())
                    .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::InvalidInput));

                if let Ok(n) = result {
                    self.cursor = n as u32;
                }
                result
            }
            SeekFrom::Start(offset) => {
                self.cursor = offset as u32;
                Ok(offset)
            }
            SeekFrom::End(_) => unimplemented!("Seek from end not yet implemented"),
        }
    }
}

impl Write for FrameBuffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let cursor = self.cursor as usize;
        FRAMEBUFFER_STATE.with(|fb| {
            let mut fb_state = fb.borrow_mut();
            match self.fb_type {
                FrameBufferFileType::Buffer => {
                    let mut bytes_copied = 0;

                    for (i, byte) in buf.iter().enumerate() {
                        if fb_state.set_byte(cursor + i, *byte).is_none() {
                            // TODO: check if we should return an error here
                            break;
                        }
                        bytes_copied += 1;
                    }

                    self.cursor += bytes_copied;
                    Ok(bytes_copied as usize)
                }
                FrameBufferFileType::Resolution => {
                    let resolution_data = format!("{}x{}", fb_state.x_size, fb_state.y_size);
                    let mut byte_vec: Vec<u8> = resolution_data.bytes().collect();
                    let upper_limit = std::cmp::min(buf.len(), byte_vec.len() - cursor as usize);

                    byte_vec[..upper_limit].clone_from_slice(&buf[..upper_limit]);

                    let mut parse_str = String::new();
                    for b in byte_vec.iter() {
                        parse_str.push(*b as char);
                    }
                    let result: Vec<&str> = parse_str.split('x').collect();
                    if result.len() != 2 {
                        return Ok(0);
                    }
                    if let Ok((n1, n2)) = result[0]
                        .parse::<u32>()
                        .and_then(|n1| result[1].parse::<u32>().map(|n2| (n1, n2)))
                    {
                        if fb_state.resize(n1, n2).is_some() {
                            return Ok(upper_limit);
                        }
                    }
                    Ok(0)
                }

                FrameBufferFileType::Draw => {
                    if buf.is_empty() {
                        Ok(0)
                    } else {
                        fb_state.draw();
                        Ok(1)
                    }
                }
                FrameBufferFileType::Input => Ok(0),
            }
        })
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
    fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        self.write(buf).map(|_| ())
    }
    fn write_fmt(&mut self, _fmt: std::fmt::Arguments) -> std::io::Result<()> {
        unimplemented!()
    }
}

#[cfg_attr(feature = "enable-serde", typetag::serde)]
impl VirtualFile for FrameBuffer {
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
        0
    }
    fn set_len(&mut self, _new_size: Filesize) -> Result<(), WasiFsError> {
        Ok(())
    }
    fn unlink(&mut self) -> Result<(), WasiFsError> {
        panic!("TODO(mark): actually implement this");
    }
    fn bytes_available(&self) -> Result<usize, WasiFsError> {
        Ok(0)
    }
}

pub fn initialize(inodes: &mut WasiInodes, fs: &mut WasiFs) -> Result<(), String> {
    let frame_buffer_file = Box::new(FrameBuffer {
        fb_type: FrameBufferFileType::Buffer,
        cursor: 0,
    });
    let resolution_file = Box::new(FrameBuffer {
        fb_type: FrameBufferFileType::Resolution,
        cursor: 0,
    });
    let draw_file = Box::new(FrameBuffer {
        fb_type: FrameBufferFileType::Draw,
        cursor: 0,
    });
    let input_file = Box::new(FrameBuffer {
        fb_type: FrameBufferFileType::Input,
        cursor: 0,
    });

    let base_dir_fd = unsafe {
        fs.open_dir_all(
            inodes,
            VIRTUAL_ROOT_FD,
            "_wasmer/dev/fb0".to_string(),
            ALL_RIGHTS,
            ALL_RIGHTS,
            0,
        )
        .map_err(|e| format!("fb: Failed to create dev folder {:?}", e))?
    };

    let _fd = fs
        .open_file_at(
            inodes,
            base_dir_fd,
            input_file,
            Fd::READ,
            "input".to_string(),
            ALL_RIGHTS,
            ALL_RIGHTS,
            0,
        )
        .map_err(|e| format!("fb: Failed to init framebuffer {:?}", e))?;

    debug!("Input open on fd {}", _fd);

    let _fd = fs
        .open_file_at(
            inodes,
            base_dir_fd,
            frame_buffer_file,
            Fd::READ | Fd::WRITE,
            "fb".to_string(),
            ALL_RIGHTS,
            ALL_RIGHTS,
            0,
        )
        .map_err(|e| format!("fb: Failed to init framebuffer {:?}", e))?;

    debug!("Framebuffer open on fd {}", _fd);

    let _fd = fs
        .open_file_at(
            inodes,
            base_dir_fd,
            resolution_file,
            Fd::READ | Fd::WRITE,
            "virtual_size".to_string(),
            ALL_RIGHTS,
            ALL_RIGHTS,
            0,
        )
        .map_err(|e| format!("fb_resolution: Failed to init framebuffer {:?}", e))?;

    debug!("Framebuffer resolution open on fd {}", _fd);

    let _fd = fs
        .open_file_at(
            inodes,
            base_dir_fd,
            draw_file,
            Fd::READ | Fd::WRITE,
            "draw".to_string(),
            ALL_RIGHTS,
            ALL_RIGHTS,
            0,
        )
        .map_err(|e| format!("fb_index_display: Failed to init framebuffer {:?}", e))?;

    debug!("Framebuffer draw open on fd {}", _fd);

    Ok(())
}
