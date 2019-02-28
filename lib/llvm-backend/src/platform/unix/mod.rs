
extern "C" {
    fn __register_frame(frame: *const u8);
    fn __deregister_frame(frame: *const u8);
}

pub unsafe fn register_eh_frames(eh_frames: *const u8, num_bytes: usize) {
    visit_frame_desc_entries(eh_frames, num_bytes, |frame| __register_frame(frame));
}

unsafe fn visit_frame_desc_entries<F>(eh_frames: *const u8, num_bytes: usize, visitor: F)
where
    F: Fn(*const u8),
{
    let mut next = eh_frames;
    let mut end = eh_frames.add(num_bytes);

    loop {
        if next >= end {
            break;
        }

        let cfi = next;
        let mut cfi_num_bytes = (next as *const u32).read_unaligned() as u64;
        assert!(cfi_num_bytes != 0);

        next = next.add(4);
        if num_bytes == 0xffffffff {
            let cfi_num_bytes64 = (next as *const u64).read_unaligned();
            cfi_num_bytes = cfi_num_bytes64;
            next = next.add(8);
        }

        let cie_offset = (next as *const u32).read_unaligned();
        if cie_offset != 0 {
            visitor(cfi);
        }
        next = next.add(cfi_num_bytes as usize);
    }
}
