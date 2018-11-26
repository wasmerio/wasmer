use libc::{
    gettimeofday,
    timeval,
    c_int,
    clock_gettime,
    clock_gettime as libc_clock_gettime,
    clockid_t,
    timespec,
    tm,
    localtime,
    time_t,
    time
};
use std::{ptr, slice, mem};
use std::time::SystemTime;


use crate::webassembly::Instance;

/// emscripten: _gettimeofday
pub extern fn _gettimeofday(tp: c_int, tz: c_int, instance: &mut Instance) -> c_int {
    #[repr(C)]
    struct GuestTimeVal {
        tv_sec: i32,
        tv_usec: i32,
    }

    assert!(tz == 0, "the timezone argument of `_gettimeofday` must be null");
    unsafe {
        let now = SystemTime::now();
        let since_epoch = now.duration_since(SystemTime::UNIX_EPOCH).unwrap();
        let timeval_struct_ptr = instance.memory_offset_addr(0, tp as _) as *mut GuestTimeVal;

        (*timeval_struct_ptr).tv_sec = since_epoch.as_secs() as _;
        (*timeval_struct_ptr).tv_usec = since_epoch.subsec_nanos() as _;
    }
    0
}


/// emscripten: _clock_gettime
pub extern fn _clock_gettime(clk_id: c_int, tp: c_int, instance: &mut Instance) -> c_int {
    #[repr(C)]
    struct GuestTimeSpec {
        tv_sec: i32,
        tv_nsec: i32,
    }

    unsafe {
        let mut timespec = timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        let ret = libc_clock_gettime(clk_id as _, &mut timespec);
        if ret != 0 {
            return ret;
        }

        let timespec_struct_ptr = instance.memory_offset_addr(0, tp as _) as *mut GuestTimeSpec;
        (*timespec_struct_ptr).tv_sec = timespec.tv_sec as _;
        (*timespec_struct_ptr).tv_nsec = timespec.tv_nsec as _;
    }
    0
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

/// emscripten: _strftime
pub extern "C" fn _strftime(s_ptr: c_int, maxsize: u32, format_ptr: c_int, tm_ptr: c_int, instance: &mut Instance) -> time_t {
    debug!("emscripten::_strftime {} {} {} {}", s_ptr, maxsize, format_ptr, tm_ptr);
    0
}
