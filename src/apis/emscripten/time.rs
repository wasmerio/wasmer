use libc::{
    gettimeofday,
    timeval,
    c_int,
    clock_gettime,
    clockid_t,
    timespec,
    tm,
    localtime,
    time_t,
    time
};
use std::{ptr, slice, mem};

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

/// emscripten: _localtime
pub extern "C" fn _localtime(time_p: u32, instance: &mut Instance) -> *mut tm {
    debug!("emscripten::_localtime {}", time_p);

    unsafe {
        let time_p_addr = instance.memory_offset_addr(0, time_p as _) as *mut i64;
        localtime(time_p_addr)
    }
}

/// emscripten: _time
pub extern "C" fn _time(time_p: u32, instance: &mut Instance) -> time_t {
    debug!("emscripten::_time {}", time_p);

    unsafe {
        let time_p_addr = instance.memory_offset_addr(0, time_p as _) as *mut i64;
        time(time_p_addr)
    }
}
