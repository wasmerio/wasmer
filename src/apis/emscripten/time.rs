use super::utils::copy_cstr_into_wasm;
use libc::{
    c_int,
    c_long,
    clock_gettime as libc_clock_gettime,
    // tm,
    localtime,
    time,
    time_t,
    timespec,
};
use std::mem;
use std::time::SystemTime;

use crate::webassembly::Instance;

/// emscripten: _gettimeofday
pub extern "C" fn _gettimeofday(tp: c_int, tz: c_int, instance: &mut Instance) -> c_int {
    debug!("emscripten::_gettimeofday {} {}", tp, tz);
    #[repr(C)]
    struct GuestTimeVal {
        tv_sec: i32,
        tv_usec: i32,
    }

    assert!(
        tz == 0,
        "the timezone argument of `_gettimeofday` must be null"
    );
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
pub extern "C" fn _clock_gettime(clk_id: c_int, tp: c_int, instance: &mut Instance) -> c_int {
    debug!("emscripten::_clock_gettime {} {}", clk_id, tp);
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
pub extern "C" fn _localtime(time_p: u32, instance: &mut Instance) -> c_int {
    debug!("emscripten::_localtime {}", time_p);

    #[repr(C)]
    struct GuestTm {
        tm_sec: i32,
        tm_min: i32,
        tm_hour: i32,
        tm_mday: i32,
        tm_mon: i32,
        tm_year: i32,
        tm_wday: i32,
        tm_yday: i32,
        tm_isdst: i32,
        tm_gmtoff: c_long,
        tm_zone: u32,
    }

    unsafe {
        let time_p_addr = instance.memory_offset_addr(0, time_p as _) as *mut i64;
        let tm_struct = &*localtime(time_p_addr);

        // Webassembly allocation
        let tm_struct_offset = (instance.emscripten_data.as_ref().unwrap().malloc)(
            mem::size_of::<GuestTm>() as _,
            instance,
        );
        let tm_struct_ptr = instance.memory_offset_addr(0, tm_struct_offset as _) as *mut GuestTm;

        // Initializing
        (*tm_struct_ptr).tm_sec = tm_struct.tm_sec;
        (*tm_struct_ptr).tm_min = tm_struct.tm_min;
        (*tm_struct_ptr).tm_hour = tm_struct.tm_hour;
        (*tm_struct_ptr).tm_mday = tm_struct.tm_mday;
        (*tm_struct_ptr).tm_mon = tm_struct.tm_mon;
        (*tm_struct_ptr).tm_year = tm_struct.tm_year;
        (*tm_struct_ptr).tm_wday = tm_struct.tm_wday;
        (*tm_struct_ptr).tm_yday = tm_struct.tm_yday;
        (*tm_struct_ptr).tm_isdst = tm_struct.tm_isdst;
        (*tm_struct_ptr).tm_gmtoff = tm_struct.tm_gmtoff;
        (*tm_struct_ptr).tm_zone = copy_cstr_into_wasm(instance, tm_struct.tm_zone);

        tm_struct_offset as c_int
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
pub extern "C" fn _strftime(
    s_ptr: c_int,
    maxsize: u32,
    format_ptr: c_int,
    tm_ptr: c_int,
    _instance: &mut Instance,
) -> time_t {
    debug!(
        "emscripten::_strftime {} {} {} {}",
        s_ptr, maxsize, format_ptr, tm_ptr
    );
    0
}
