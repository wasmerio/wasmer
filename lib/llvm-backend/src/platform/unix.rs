/// `__register_frame` and `__deregister_frame` on macos take a single fde as an
/// argument, so we need to parse the fde table here.
///
/// This is a pretty direct port of llvm's fde handling code:
///     https://llvm.org/doxygen/RTDyldMemoryManager_8cpp_source.html.
#[cfg(target_os = "macos")]
pub unsafe fn visit_fde(addr: *mut u8, size: usize, visitor: extern "C" fn(*mut u8)) {
    unsafe fn process_fde(entry: *mut u8, visitor: extern "C" fn(*mut u8)) -> *mut u8 {
        let mut p = entry;
        let length = (p as *const u32).read_unaligned();
        p = p.add(4);
        let offset = (p as *const u32).read_unaligned();

        if offset != 0 {
            visitor(entry);
        }
        p.add(length as usize)
    }

    let mut p = addr;
    let end = p.add(size);

    loop {
        if p >= end {
            break;
        }

        p = process_fde(p, visitor);
    }
}

#[cfg(not(target_os = "macos"))]
pub unsafe fn visit_fde(addr: *mut u8, size: usize, visitor: extern "C" fn(*mut u8)) {
    visitor(addr);
}
