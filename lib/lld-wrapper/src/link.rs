use std::os::raw::c_char;

extern "C" {
    pub fn wasmer_lld_wrapper_macho_link(
        object_starts: *const *const c_char,
        object_lengths: *const u32,
    );
    pub fn wasmer_lld_wrapper_link(filenames: *const *const c_char, count: u32);
}
