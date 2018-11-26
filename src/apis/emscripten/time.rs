use libc::{gettimeofday, timeval, c_int, clock_gettime, clockid_t, timespec};
use std::ptr;

use crate::webassembly::Instance;

/// emscripten: _gettimeofday
pub extern "C" fn _gettimeofday(timeval_ptr_offset: c_int, tz_offset: c_int, instance: &mut Instance) -> c_int {
    debug!("emscripten::_gettimeofday {}", timeval_ptr_offset);

    unsafe {
        let mut timeval_value = *(instance.memory_offset_addr(0, timeval_ptr_offset as _) as *mut timeval);
        // We skip the timezone for now
        let mut tz = ptr::null_mut();
        debug!("emscripten::_gettimeofday(initial) {} {}", (timeval_value).tv_sec, (timeval_value).tv_usec);

        let returned = gettimeofday(&mut timeval_value, tz);
        debug!("emscripten::_gettimeofday(filled) {} {}", (timeval_value).tv_sec, (timeval_value).tv_usec);
        returned
    }
}

/// emscripten: _clock_gettime
pub extern "C" fn _clock_gettime(clk_id: clockid_t, tp_offset: c_int, instance: &mut Instance) -> c_int {
    debug!("emscripten::_clock_gettime {} {}", clk_id, tp_offset);

    unsafe {
        let mut tp = instance.memory_offset_addr(0, tp_offset as _) as *mut timespec;
        let returned = clock_gettime(clk_id, tp);
        debug!("emscripten::clock_gettime(filled) {} {}", (*tp).tv_sec, (*tp).tv_nsec);
        returned
    }
}
