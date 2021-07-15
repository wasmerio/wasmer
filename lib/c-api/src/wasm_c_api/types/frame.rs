use super::super::instance::wasm_instance_t;
use wasmer_api::FrameInfo;

#[allow(non_camel_case_types)]
#[derive(Debug, Clone)]
pub struct wasm_frame_t {
    info: FrameInfo,
}

impl<'a> From<&'a FrameInfo> for wasm_frame_t {
    fn from(other: &'a FrameInfo) -> Self {
        other.clone().into()
    }
}

impl From<FrameInfo> for wasm_frame_t {
    fn from(other: FrameInfo) -> Self {
        Self { info: other }
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_frame_copy(frame: &wasm_frame_t) -> Box<wasm_frame_t> {
    Box::new(frame.clone())
}

#[no_mangle]
pub unsafe extern "C" fn wasm_frame_delete(_frame: Option<Box<wasm_frame_t>>) {}

#[no_mangle]
pub unsafe extern "C" fn wasm_frame_instance(_frame: &wasm_frame_t) -> *const wasm_instance_t {
    std::ptr::null()
}

#[no_mangle]
pub unsafe extern "C" fn wasm_frame_func_index(frame: &wasm_frame_t) -> u32 {
    frame.info.func_index()
}

#[no_mangle]
pub unsafe extern "C" fn wasm_frame_func_offset(frame: &wasm_frame_t) -> usize {
    frame.info.func_offset()
}

#[no_mangle]
pub unsafe extern "C" fn wasm_frame_module_offset(frame: &wasm_frame_t) -> usize {
    frame.info.module_offset()
}

wasm_declare_boxed_vec!(frame);
