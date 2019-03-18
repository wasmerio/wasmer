use std::{
    alloc::{alloc, Layout},
    mem::{align_of, size_of},
    slice,
};
use wasmer_runtime_core::types::FuncIndex;

pub struct CodeObject {
    header: *mut Header,
}

impl CodeObject {
    pub fn new(code_size: u32, call_offsets: &[CallOffset]) -> Self {
        unsafe {
            let layout = Layout::from_size_align_unchecked(
                size_of::<Header>()
                    + (call_offsets.len() * size_of::<CallOffset>())
                    + code_size as usize,
                align_of::<usize>(),
            );
            let header = alloc(layout) as *mut Header;
            assert!(!header.is_null());

            (*header).code_size = code_size;
            (*header).call_offsets_len = call_offsets.len() as u32;
            call_offsets
                .as_ptr()
                .copy_to((*header).call_offsets.as_mut_ptr(), call_offsets.len());

            Self { header }
        }
    }

    pub fn call_offsets(&self) -> &[CallOffset] {
        unsafe {
            let call_offsets_len = (*self.header).call_offsets_len as usize;
            slice::from_raw_parts((*self.header).call_offsets.as_ptr(), call_offsets_len)
        }
    }

    pub fn code_ptr(&self) -> *const u8 {
        unsafe {
            let call_offsets_len = (*self.header).call_offsets_len as usize;
            ((&(*self.header).call_offsets) as *const CallOffset).add(call_offsets_len) as *const u8
        }
    }
}

#[repr(C)]
pub struct CallOffset {
    func_index: FuncIndex,
    offset: u32,
}

#[repr(C)]
struct Header {
    code_size: u32,
    call_offsets_len: u32,
    call_offsets: [CallOffset; 0],
}
