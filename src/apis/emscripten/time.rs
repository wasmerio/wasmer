use super::utils::copy_cstr_into_wasm;
use libc::{
    c_int,
    c_long,
    clock_gettime as libc_clock_gettime,
    localtime,
    localtime_r,
    tm,
    time,
    time_t,
    timespec,
    c_char,
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

#[repr(C)]
struct guest_tm {
    pub tm_sec: c_int, // 0
    pub tm_min: c_int, // 4
    pub tm_hour: c_int, // 8
    pub tm_mday: c_int, // 12
    pub tm_mon: c_int, // 16
    pub tm_year: c_int, // 20
    pub tm_wday: c_int, // 24
    pub tm_yday: c_int, // 28
    pub tm_isdst: c_int, // 32
    pub tm_gmtoff: c_int, // 36
    pub tm_zone: c_int, // 40
}

/// emscripten: _asctime
pub extern "C" fn _asctime(time: u32, instance: &mut Instance) -> u32 {
    debug!("emscripten::_asctime {}", time);

    unsafe {
        let date = &*(instance.memory_offset_addr(0, time as _) as *mut guest_tm);

        let days = vec!["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
        let months = vec!["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

        let day = if date.tm_mday < 10 {"  "} else {" "};
        let hour = if date.tm_hour < 10 {" 0"} else {" "};
        let min = if date.tm_min < 10 {":0"} else {":"};
        let sec = if date.tm_sec < 10 {":0"} else {":"};
        let year = 1900 + date.tm_year;

        let mut time_str = format!(
            // NOTE: The 14 accompanying chars are needed for some reason
            "{} {}{}{}{}{}{}{}{}{} {}\0\0\0\0\0\0\0\0\0\0\0\0\0\0",
            days[date.tm_wday as usize],
            months[date.tm_mon as usize],
            day, date.tm_mday,
            hour, date.tm_hour,
            min, date.tm_min,
            sec, date.tm_sec,
            year
        );


        // NOTE: asctime_r is specced to behave in an undefined manner if the algorithm would attempt
        //      to write out more than 26 bytes (including the null terminator).
        //      See http://pubs.opengroup.org/onlinepubs/9699919799/functions/asctime.html
        //      Our undefined behavior is to truncate the write to at most 26 bytes, including null terminator.
        let time_str_ptr = time_str[0..26].as_ptr() as _;
        let time_str_offset = copy_cstr_into_wasm(instance, time_str_ptr);

        let c_str = instance.memory_offset_addr(0, time_str_offset as _) as *mut i8;
        use std::ffi::CStr;

        // std::mem::forget(time_str);
        time_str_offset
    }
}

/// emscripten: _tvset
pub extern "C" fn _tvset() {
    debug!("emscripten::_tvset UNIMPLEMENTED");
}

/// emscripten: _localtime
pub extern "C" fn _localtime(time_p: u32, instance: &mut Instance) -> c_int {
    debug!("emscripten::_localtime {}", time_p);
    // NOTE: emscripten seems to want tzset() called in this function
    //      https://stackoverflow.com/questions/19170721/real-time-awareness-of-timezone-change-in-localtime-vs-localtime-r

    unsafe {
        let time_p_addr = instance.memory_offset_addr(0, time_p as _) as *mut i64;

        let tm_struct = &*localtime(time_p_addr);

        let tm_struct_offset = (instance.emscripten_data.as_ref().unwrap().malloc)(
            mem::size_of::<guest_tm>() as _,
            instance,
        );

        let tm_struct_ptr = instance.memory_offset_addr(0, tm_struct_offset as _) as *mut guest_tm;

        (*tm_struct_ptr).tm_sec = tm_struct.tm_sec;
        (*tm_struct_ptr).tm_min = tm_struct.tm_min;
        (*tm_struct_ptr).tm_hour = tm_struct.tm_hour;
        (*tm_struct_ptr).tm_mday = tm_struct.tm_mday;
        (*tm_struct_ptr).tm_mon = tm_struct.tm_mon;
        (*tm_struct_ptr).tm_year = tm_struct.tm_year;
        (*tm_struct_ptr).tm_wday = tm_struct.tm_wday;
        (*tm_struct_ptr).tm_yday = tm_struct.tm_yday;
        (*tm_struct_ptr).tm_isdst = tm_struct.tm_isdst;
        (*tm_struct_ptr).tm_gmtoff = tm_struct.tm_gmtoff as i32;
        (*tm_struct_ptr).tm_zone = copy_cstr_into_wasm(instance, tm_struct.tm_zone) as i32;

        tm_struct_offset as c_int
    }
}
/// emscripten: _localtime_r
pub extern "C" fn _localtime_r(time_p: u32, result: u32, instance: &mut Instance) -> c_int {
    debug!("emscripten::_localtime_r {}", time_p);

    // NOTE: emscripten seems to want tzset() called in this function
    //      https://stackoverflow.com/questions/19170721/real-time-awareness-of-timezone-change-in-localtime-vs-localtime-r

    unsafe {
        let time_p_addr = instance.memory_offset_addr(0, time_p as _) as *mut i64;
        let result_addr = instance.memory_offset_addr(0, result as _) as *mut guest_tm;
        let mut result_tm = tm {
            tm_sec: (*result_addr).tm_sec,
            tm_min: (*result_addr).tm_min,
            tm_hour: (*result_addr).tm_hour,
            tm_mday: (*result_addr).tm_mday,
            tm_mon: (*result_addr).tm_mon,
            tm_year: (*result_addr).tm_year,
            tm_wday: (*result_addr).tm_wday,
            tm_yday: (*result_addr).tm_yday,
            tm_isdst: (*result_addr).tm_isdst,
            tm_gmtoff: (*result_addr).tm_gmtoff as _,
            tm_zone: instance.memory_offset_addr(0, (*result_addr).tm_zone as _) as _,
        };

        let tm_struct = &*localtime_r(time_p_addr, &mut result_tm);

        let tm_struct_offset = (instance.emscripten_data.as_ref().unwrap().malloc)(
            mem::size_of::<guest_tm>() as _,
            instance,
        );

        let tm_struct_ptr = instance.memory_offset_addr(0, tm_struct_offset as _) as *mut guest_tm;

        (*tm_struct_ptr).tm_sec = tm_struct.tm_sec;
        (*tm_struct_ptr).tm_min = tm_struct.tm_min;
        (*tm_struct_ptr).tm_hour = tm_struct.tm_hour;
        (*tm_struct_ptr).tm_mday = tm_struct.tm_mday;
        (*tm_struct_ptr).tm_mon = tm_struct.tm_mon;
        (*tm_struct_ptr).tm_year = tm_struct.tm_year;
        (*tm_struct_ptr).tm_wday = tm_struct.tm_wday;
        (*tm_struct_ptr).tm_yday = tm_struct.tm_yday;
        (*tm_struct_ptr).tm_isdst = tm_struct.tm_isdst;
        (*tm_struct_ptr).tm_gmtoff = tm_struct.tm_gmtoff as i32;
        (*tm_struct_ptr).tm_zone = copy_cstr_into_wasm(instance, tm_struct.tm_zone) as i32;

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
