use std::convert::TryInto;
use std::ffi::{CString, OsStr};
use std::os::raw::c_char;
use std::path::Path;

#[cfg(target_family = "unix")]
use std::os::unix::ffi::OsStrExt;

mod link;

pub unsafe fn link_paths(filenames: &[&Path]) {
    let mut storage: Vec<CString> = Vec::new();
    for f in filenames {
        #[cfg(target_family = "unix")]
        let cstring = CString::new(OsStr::new(f.as_os_str()).as_bytes()).unwrap();
        #[cfg(target_family = "windows")]
        let cstring = CString::new(OsStr::new(f.as_os_str()).to_str().unwrap()).unwrap();
        storage.push(cstring);
    }
    let mut ptrs: Vec<*const c_char> = Vec::new();
    for s in &storage {
        ptrs.push(s.as_ptr());
    }
    link::wasmer_lld_wrapper_link(ptrs.as_ptr(), ptrs.len().try_into().unwrap());
}

/*
#[test]
fn my_test() {
    unsafe { link_paths(&[Path::new("/tmp/a.o"), Path::new("/tmp/b.o")]) };
}
 */

/*
#[test]
fn my_test() {
    let filenames = [b"/tmp/a.o", std::ptr::null()];
    let lengths = [8u32, 0u32];
    unsafe {
        link::wasmer_lld_wrapper_macho_link(
            filenames.as_ptr() as *const *const c_char,
            lengths.as_ptr() as *const u32,
        );
    }
}
*/
