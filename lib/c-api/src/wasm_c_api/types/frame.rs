use super::super::instance::wasm_instance_t;
use libc::c_char;
use std::ffi::CString;
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

#[repr(C)]
#[allow(non_camel_case_types)]
#[derive(Debug, Clone)]
pub struct wasm_name_t {
    pub name: *mut c_char,
}

#[no_mangle]
pub unsafe extern "C" fn wasm_frame_module_name(frame: &wasm_frame_t) -> wasm_name_t {
    let module_name =
        Some(frame.info.module_name()).and_then(|f| Some(CString::new(f).ok()?.into_raw()));

    match module_name {
        Some(s) => wasm_name_t { name: s },
        None => wasm_name_t {
            name: core::ptr::null_mut(),
        },
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_frame_func_name(frame: &wasm_frame_t) -> wasm_name_t {
    let func_name = frame
        .info
        .function_name()
        .and_then(|f| Some(CString::new(f).ok()?.into_raw()));

    match func_name {
        Some(s) => wasm_name_t { name: s },
        None => wasm_name_t {
            name: core::ptr::null_mut(),
        },
    }
}

#[no_mangle]
pub unsafe extern "C" fn wasm_name_delete(name: Option<&mut wasm_name_t>) {
    if let Some(s) = name {
        if !s.name.is_null() {
            let _ = CString::from_raw(s.name);
        }
    }
}

wasm_declare_boxed_vec!(frame);

#[cfg(test)]
#[test]
fn test_frame_name() {
    use std::ffi::CStr;
    use wasmer_types::SourceLoc;

    let info = wasm_frame_t {
        info: FrameInfo::new(
            "module_name".to_string(),
            5,
            Some("function_name".to_string()),
            SourceLoc::new(10),
            SourceLoc::new(20),
        ),
    };

    unsafe {
        let mut wasm_frame_func_name = wasm_frame_func_name(&info);
        let s = CStr::from_ptr(wasm_frame_func_name.name);
        assert_eq!(s.to_str().unwrap(), "function_name");
        wasm_name_delete(Some(&mut wasm_frame_func_name));

        let mut wasm_frame_module_name = wasm_frame_module_name(&info);
        let s = CStr::from_ptr(wasm_frame_module_name.name);
        assert_eq!(s.to_str().unwrap(), "module_name");
        wasm_name_delete(Some(&mut wasm_frame_module_name));
    }

    println!("{:#?}", info);
}
