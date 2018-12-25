use crate::webassembly::module::Module;
use crate::webassembly::Instance;
use byteorder::{ByteOrder, LittleEndian};
use libc::stat;
use std::ffi::CStr;
use std::mem::size_of;
use std::os::raw::c_char;
use std::slice;

/// We check if a provided module is an Emscripten generated one
pub fn is_emscripten_module(module: &Module) -> bool {
    for (module, field) in &module.info.imported_funcs {
        if field == "_emscripten_memcpy_big" && module == "env" {
            return true;
        }
    }
    false
}

pub unsafe fn write_to_buf(string: *const c_char, buf: u32, max: u32, instance: &Instance) -> u32 {
    let buf_addr = instance.memory_offset_addr(0, buf as _) as *mut c_char;

    for i in 0..max {
        *buf_addr.add(i as _) = *string.add(i as _);
    }

    buf
}

/// This function expects nullbyte to be appended.
pub unsafe fn copy_cstr_into_wasm(instance: &mut Instance, cstr: *const c_char) -> u32 {
    let s = CStr::from_ptr(cstr).to_str().unwrap();
    let cstr_len = s.len();
    let space_offset =
        (instance.emscripten_data.as_ref().unwrap().malloc)((cstr_len as i32) + 1, instance);
    let raw_memory = instance.memory_offset_addr(0, space_offset as _) as *mut u8;
    let slice = slice::from_raw_parts_mut(raw_memory, cstr_len);

    for (byte, loc) in s.bytes().zip(slice.iter_mut()) {
        *loc = byte;
    }

    // TODO: Appending null byte won't work, because there is CStr::from_ptr(cstr)
    //      at the top that crashes when there is no null byte
    *raw_memory.add(cstr_len) = 0;

    space_offset
}

pub unsafe fn allocate_on_stack<'a, T: Copy>(
    count: u32,
    instance: &'a Instance,
) -> (u32, &'a mut [T]) {
    let offset = (instance.emscripten_data.as_ref().unwrap().stack_alloc)(
        count * (size_of::<T>() as u32),
        instance,
    );
    let addr = instance.memory_offset_addr(0, offset as _) as *mut T;
    let slice = slice::from_raw_parts_mut(addr, count as usize);

    (offset, slice)
}

pub unsafe fn allocate_cstr_on_stack<'a>(s: &str, instance: &'a Instance) -> (u32, &'a [u8]) {
    let (offset, slice) = allocate_on_stack((s.len() + 1) as u32, instance);

    use std::iter;
    for (byte, loc) in s.bytes().chain(iter::once(0)).zip(slice.iter_mut()) {
        *loc = byte;
    }

    (offset, slice)
}

pub unsafe fn copy_terminated_array_of_cstrs(
    _instance: &mut Instance,
    cstrs: *mut *mut c_char,
) -> u32 {
    let total_num = {
        let mut ptr = cstrs;
        let mut counter = 0;
        while !(*ptr).is_null() {
            counter += 1;
            ptr = ptr.add(1);
        }
        counter
    };
    debug!(
        "emscripten::copy_terminated_array_of_cstrs::total_num: {}",
        total_num
    );
    0
}

pub unsafe fn copy_stat_into_wasm(instance: &mut Instance, buf: u32, stat: &stat) {
    let buf_ptr = instance.memory_offset_addr(0, buf as _) as *mut u8;
    let buf = slice::from_raw_parts_mut(buf_ptr, 76);

    LittleEndian::write_u32(&mut buf[..], stat.st_dev as _);
    LittleEndian::write_u32(&mut buf[4..], 0);
    LittleEndian::write_u32(&mut buf[8..], stat.st_ino as _);
    LittleEndian::write_u32(&mut buf[12..], stat.st_mode as _);
    LittleEndian::write_u32(&mut buf[16..], stat.st_nlink as _);
    LittleEndian::write_u32(&mut buf[20..], stat.st_uid);
    LittleEndian::write_u32(&mut buf[24..], stat.st_gid);
    LittleEndian::write_u32(&mut buf[28..], stat.st_rdev as _);
    LittleEndian::write_u32(&mut buf[32..], 0);
    LittleEndian::write_u32(&mut buf[36..], stat.st_size as _);
    LittleEndian::write_u32(&mut buf[40..], 4096);
    LittleEndian::write_u32(&mut buf[44..], stat.st_blocks as _);
    LittleEndian::write_u32(&mut buf[48..], stat.st_atime as _);
    LittleEndian::write_u32(&mut buf[52..], 0);
    LittleEndian::write_u32(&mut buf[56..], stat.st_mtime as _);
    LittleEndian::write_u32(&mut buf[60..], 0);
    LittleEndian::write_u32(&mut buf[64..], stat.st_ctime as _);
    LittleEndian::write_u32(&mut buf[68..], 0);
    LittleEndian::write_u32(&mut buf[72..], stat.st_ino as _);
}

#[cfg(test)]
mod tests {
    use super::is_emscripten_module;
    use crate::webassembly::compile;

    #[test]
    fn should_detect_emscripten_files() {
        let wasm_bytes = include_wast2wasm_bytes!("tests/is_emscripten_true.wast");
        let module = compile(wasm_bytes).expect("Not compiled properly");
        assert!(is_emscripten_module(&module));
    }

    #[test]
    fn should_detect_non_emscripten_files() {
        let wasm_bytes = include_wast2wasm_bytes!("tests/is_emscripten_false.wast");
        let module = compile(wasm_bytes).expect("Not compiled properly");
        assert!(!is_emscripten_module(&module));
    }
}
