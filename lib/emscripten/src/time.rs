use super::utils::{copy_cstr_into_wasm, write_to_buf};
use crate::{allocate_on_stack, lazy_static, EmEnv};
use libc::{c_char, c_int};
// use libc::{c_char, c_int, clock_getres, clock_settime};
use std::mem;
use std::time::SystemTime;

#[cfg(not(target_os = "windows"))]
use libc::{clockid_t, time as libc_time, timegm as libc_timegm, tm as libc_tm};
#[cfg(not(target_os = "windows"))]
use std::ffi::CString;

#[cfg(target_os = "windows")]
use libc::time_t;

use wasmer::FunctionEnvMut;

#[cfg(target_os = "windows")]
#[allow(non_camel_case_types)]
type clockid_t = c_int;

#[cfg(target_os = "windows")]
extern "C" {
    #[link_name = "time"]
    pub fn libc_time(s: *const time_t) -> time_t;
}

use super::env;

#[cfg(any(target_os = "linux", target_os = "android"))]
use libc::{CLOCK_MONOTONIC, CLOCK_MONOTONIC_COARSE, CLOCK_REALTIME};

#[cfg(target_os = "freebsd")]
use libc::{CLOCK_MONOTONIC, CLOCK_REALTIME};
#[cfg(target_os = "freebsd")]
const CLOCK_MONOTONIC_COARSE: clockid_t = 6;

#[cfg(target_vendor = "apple")]
use libc::CLOCK_REALTIME;
#[cfg(target_vendor = "apple")]
const CLOCK_MONOTONIC: clockid_t = 1;
#[cfg(target_vendor = "apple")]
const CLOCK_MONOTONIC_COARSE: clockid_t = 6;

// some assumptions about the constants when targeting windows
#[cfg(target_os = "windows")]
const CLOCK_REALTIME: clockid_t = 0;
#[cfg(target_os = "windows")]
const CLOCK_MONOTONIC: clockid_t = 1;
#[cfg(target_os = "windows")]
const CLOCK_MONOTONIC_COARSE: clockid_t = 6;

/// emscripten: _gettimeofday
#[allow(clippy::cast_ptr_alignment)]
pub fn _gettimeofday(ctx: FunctionEnvMut<EmEnv>, tp: c_int, tz: c_int) -> c_int {
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
        let memory = ctx.data().memory(0);
        let timeval_struct_ptr =
            emscripten_memory_pointer!(memory.view(&ctx), tp) as *mut GuestTimeVal;

        (*timeval_struct_ptr).tv_sec = since_epoch.as_secs() as _;
        (*timeval_struct_ptr).tv_usec = since_epoch.subsec_nanos() as _;
    }
    0
}

pub fn _clock_getres(mut _ctx: FunctionEnvMut<EmEnv>, _clk_id: i32, _tp: i32) -> i32 {
    debug!("emscripten::_clock_getres");
    // clock_getres(clk_id, tp)
    0
}

/// emscripten: _clock_gettime
#[allow(clippy::cast_ptr_alignment)]
pub fn _clock_gettime(ctx: FunctionEnvMut<EmEnv>, clk_id: clockid_t, tp: c_int) -> c_int {
    debug!("emscripten::_clock_gettime {} {}", clk_id, tp);
    // debug!("Memory {:?}", ctx.memory(0)[..]);
    #[repr(C)]
    struct GuestTimeSpec {
        tv_sec: i32,
        tv_nsec: i32,
    }

    #[allow(unreachable_patterns)]
    let duration = match clk_id {
        CLOCK_REALTIME => time::OffsetDateTime::now_utc().unix_timestamp_nanos(),

        CLOCK_MONOTONIC | CLOCK_MONOTONIC_COARSE => {
            lazy_static! {
                static ref PRECISE0: time::Instant = time::Instant::now();
            };
            let precise_ns = *PRECISE0;
            (time::Instant::now() - precise_ns).whole_nanoseconds()
        }
        _ => panic!("Clock with id \"{}\" is not supported.", clk_id),
    };

    unsafe {
        let memory = ctx.data().memory(0);
        let timespec_struct_ptr =
            emscripten_memory_pointer!(memory.view(&ctx), tp) as *mut GuestTimeSpec;
        (*timespec_struct_ptr).tv_sec = (duration / 1_000_000_000) as _;
        (*timespec_struct_ptr).tv_nsec = (duration % 1_000_000_000) as _;
    }
    0
}

pub fn _clock_settime(mut _ctx: FunctionEnvMut<EmEnv>, _clk_id: i32, _tp: i32) -> i32 {
    debug!("emscripten::_clock_settime");
    // clock_settime(clk_id, tp)
    0
}

/// emscripten: ___clock_gettime
pub fn ___clock_gettime(ctx: FunctionEnvMut<EmEnv>, clk_id: clockid_t, tp: c_int) -> c_int {
    debug!("emscripten::___clock_gettime {} {}", clk_id, tp);
    _clock_gettime(ctx, clk_id, tp)
}

/// emscripten: _clock
pub fn _clock(mut _ctx: FunctionEnvMut<EmEnv>) -> c_int {
    debug!("emscripten::_clock");
    0 // TODO: unimplemented
}

/// emscripten: _difftime
pub fn _difftime(mut _ctx: FunctionEnvMut<EmEnv>, t0: u32, t1: u32) -> f64 {
    debug!("emscripten::_difftime");
    (t0 - t1) as _
}

pub fn _gmtime_r(mut _ctx: FunctionEnvMut<EmEnv>, _one: i32, _two: i32) -> i32 {
    debug!("emscripten::_gmtime_r");
    -1
}

pub fn _mktime(mut _ctx: FunctionEnvMut<EmEnv>, _one: i32) -> i32 {
    debug!("emscripten::_mktime");
    -1
}

pub fn _gmtime(mut _ctx: FunctionEnvMut<EmEnv>, _one: i32) -> i32 {
    debug!("emscripten::_gmtime");
    -1
}

#[repr(C)]
struct guest_tm {
    pub tm_sec: c_int,    // 0
    pub tm_min: c_int,    // 4
    pub tm_hour: c_int,   // 8
    pub tm_mday: c_int,   // 12
    pub tm_mon: c_int,    // 16
    pub tm_year: c_int,   // 20
    pub tm_wday: c_int,   // 24
    pub tm_yday: c_int,   // 28
    pub tm_isdst: c_int,  // 32
    pub tm_gmtoff: c_int, // 36
    pub tm_zone: c_int,   // 40
}

/// emscripten: _tvset
pub fn _tvset(mut _ctx: FunctionEnvMut<EmEnv>) {
    debug!("emscripten::_tvset UNIMPLEMENTED");
}

/// formats time as a C string
#[allow(clippy::cast_ptr_alignment)]
unsafe fn fmt_time(ctx: FunctionEnvMut<EmEnv>, time: u32) -> *const c_char {
    let memory = ctx.data().memory(0);
    let date = &*(emscripten_memory_pointer!(memory.view(&ctx), time) as *mut guest_tm);

    let days = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let months = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let year = 1900 + date.tm_year;

    let time_str = format!(
        // NOTE: TODO: Hack! The 14 accompanying chars are needed for some reason
        "{} {} {:2} {:02}:{:02}:{:02} {:4}\n\0\0\0\0\0\0\0\0\0\0\0\0\0",
        days[date.tm_wday as usize],
        months[date.tm_mon as usize],
        date.tm_mday,
        date.tm_hour,
        date.tm_min,
        date.tm_sec,
        year
    );

    time_str[0..26].as_ptr() as _
}

/// emscripten: _asctime
pub fn _asctime(mut ctx: FunctionEnvMut<EmEnv>, time: u32) -> u32 {
    debug!("emscripten::_asctime {}", time);

    unsafe {
        let time_str_ptr = fmt_time(ctx.as_mut(), time);
        copy_cstr_into_wasm(&mut ctx, time_str_ptr)

        // let c_str = emscripten_memory_pointer!(ctx, ctx.data().memory(0), res) as *mut i8;
        // use std::ffi::CStr;
        // debug!("#### cstr = {:?}", CStr::from_ptr(c_str));
    }
}

/// emscripten: _asctime_r
pub fn _asctime_r(mut ctx: FunctionEnvMut<EmEnv>, time: u32, buf: u32) -> u32 {
    debug!("emscripten::_asctime_r {}, {}", time, buf);

    unsafe {
        // NOTE: asctime_r is specced to behave in an undefined manner if the algorithm would attempt
        //      to write out more than 26 bytes (including the null terminator).
        //      See http://pubs.opengroup.org/onlinepubs/9699919799/functions/asctime.html
        //      Our undefined behavior is to truncate the write to at most 26 bytes, including null terminator.
        let time_str_ptr = fmt_time(ctx.as_mut(), time);
        write_to_buf(ctx, time_str_ptr, buf, 26)

        // let c_str = emscripten_memory_pointer!(ctx, ctx.data().memory(0), res) as *mut i8;
        // use std::ffi::CStr;
        // debug!("#### cstr = {:?}", CStr::from_ptr(c_str));
    }
}

/// emscripten: _localtime
#[allow(clippy::cast_ptr_alignment)]
pub fn _localtime(mut ctx: FunctionEnvMut<EmEnv>, time_p: u32) -> c_int {
    debug!("emscripten::_localtime {}", time_p);
    // NOTE: emscripten seems to want tzset() called in this function
    //      https://stackoverflow.com/questions/19170721/real-time-awareness-of-timezone-change-in-localtime-vs-localtime-r

    let memory = ctx.data().memory(0);
    let timespec = unsafe {
        let time_p_addr = emscripten_memory_pointer!(memory.view(&ctx), time_p) as *mut i64;
        let seconds = *time_p_addr;
        time::OffsetDateTime::from_unix_timestamp(seconds).unwrap()
    };

    unsafe {
        let tm_struct_offset = env::call_malloc(&mut ctx, mem::size_of::<guest_tm>() as _);
        let tm_struct_ptr =
            emscripten_memory_pointer!(memory.view(&ctx), tm_struct_offset) as *mut guest_tm;
        // debug!(
        //     ">>>>>>> time = {}, {}, {}, {}, {}, {}, {}, {}",
        //     result_tm.tm_sec, result_tm.tm_min, result_tm.tm_hour, result_tm.tm_mday,
        //     result_tm.tm_mon, result_tm.tm_year, result_tm.tm_wday, result_tm.tm_yday,
        // );
        (*tm_struct_ptr).tm_sec = timespec.second() as _;
        (*tm_struct_ptr).tm_min = timespec.minute() as _;
        (*tm_struct_ptr).tm_hour = timespec.hour() as _;
        (*tm_struct_ptr).tm_mon = timespec.month() as _;
        (*tm_struct_ptr).tm_mday = timespec.day() as _;
        (*tm_struct_ptr).tm_year = timespec.year();
        (*tm_struct_ptr).tm_wday = timespec.weekday() as _;
        (*tm_struct_ptr).tm_yday = timespec.ordinal() as _;
        (*tm_struct_ptr).tm_isdst = -1; // DST information unknown with time 0.2+
        (*tm_struct_ptr).tm_gmtoff = 0;
        (*tm_struct_ptr).tm_zone = 0;

        tm_struct_offset as _
    }
}
/// emscripten: _localtime_r
#[allow(clippy::cast_ptr_alignment)]
pub fn _localtime_r(ctx: FunctionEnvMut<EmEnv>, time_p: u32, result: u32) -> c_int {
    debug!("emscripten::_localtime_r {}", time_p);

    // NOTE: emscripten seems to want tzset() called in this function
    //      https://stackoverflow.com/questions/19170721/real-time-awareness-of-timezone-change-in-localtime-vs-localtime-r

    let memory = ctx.data().memory(0);
    unsafe {
        let seconds = emscripten_memory_pointer!(memory.view(&ctx), time_p) as *const i32;
        let timespec = time::OffsetDateTime::from_unix_timestamp_nanos(*seconds as _).unwrap();

        // debug!(
        //     ">>>>>>> time = {}, {}, {}, {}, {}, {}, {}, {}",
        //     result_tm.tm_sec, result_tm.tm_min, result_tm.tm_hour, result_tm.tm_mday,
        //     result_tm.tm_mon, result_tm.tm_year, result_tm.tm_wday, result_tm.tm_yday,
        // );

        let result_addr = emscripten_memory_pointer!(memory.view(&ctx), result) as *mut guest_tm;

        (*result_addr).tm_sec = timespec.second() as _;
        (*result_addr).tm_min = timespec.minute() as _;
        (*result_addr).tm_hour = timespec.hour() as _;
        (*result_addr).tm_mon = timespec.month() as _;
        (*result_addr).tm_mday = timespec.day() as _;
        (*result_addr).tm_year = timespec.year();
        (*result_addr).tm_wday = timespec.weekday() as _;
        (*result_addr).tm_yday = timespec.ordinal() as _;
        (*result_addr).tm_isdst = -1; // DST information unknown with time 0.2+
        (*result_addr).tm_gmtoff = 0;
        (*result_addr).tm_zone = 0;

        result as _
    }
}

/// emscripten: _time
#[allow(clippy::cast_ptr_alignment)]
pub fn _time(ctx: FunctionEnvMut<EmEnv>, time_p: u32) -> i32 {
    debug!("emscripten::_time {}", time_p);

    unsafe {
        let memory = ctx.data().memory(0);
        let time_p_addr = emscripten_memory_pointer!(memory.view(&ctx), time_p) as *mut i64;
        libc_time(time_p_addr) as i32 // TODO review i64
    }
}

pub fn _ctime_r(mut ctx: FunctionEnvMut<EmEnv>, time_p: u32, buf: u32) -> u32 {
    debug!("emscripten::_ctime_r {} {}", time_p, buf);

    // var stack = stackSave();
    let (result_offset, _result_slice): (u32, &mut [u8]) =
        unsafe { allocate_on_stack(&mut ctx, 44) };
    let time = _localtime_r(ctx.as_mut(), time_p, result_offset) as u32;
    _asctime_r(ctx, time, buf)
    // stackRestore(stack);
}

pub fn _ctime(ctx: FunctionEnvMut<EmEnv>, time_p: u32) -> u32 {
    debug!("emscripten::_ctime {}", time_p);
    let tm_current = 2414544;
    _ctime_r(ctx, time_p, tm_current)
}

/// emscripten: _timegm
#[cfg(not(target_os = "windows"))]
#[allow(clippy::cast_ptr_alignment)]
pub fn _timegm(ctx: FunctionEnvMut<EmEnv>, time_ptr: u32) -> i32 {
    debug!("emscripten::_timegm {}", time_ptr);

    unsafe {
        let memory = ctx.data().memory(0);
        let time_p_addr = emscripten_memory_pointer!(memory.view(&ctx), time_ptr) as *mut guest_tm;

        let x: *mut c_char = CString::new("").expect("CString::new failed").into_raw();
        let mut rust_tm = libc_tm {
            tm_sec: 0,
            tm_min: 0,
            tm_hour: 0,
            tm_mday: 0,
            tm_mon: 0,
            tm_year: 0,
            tm_wday: 0,
            tm_yday: 0,
            tm_isdst: 0,
            tm_gmtoff: 0,
            tm_zone: x,
        };

        let result = libc_timegm(&mut rust_tm) as i32;
        if result != 0 {
            (*time_p_addr).tm_sec = rust_tm.tm_sec;
            (*time_p_addr).tm_min = rust_tm.tm_min;
            (*time_p_addr).tm_hour = rust_tm.tm_hour;
            (*time_p_addr).tm_mday = rust_tm.tm_mday;
            (*time_p_addr).tm_mon = rust_tm.tm_mon;
            (*time_p_addr).tm_year = rust_tm.tm_year;
            (*time_p_addr).tm_wday = rust_tm.tm_wday;
            (*time_p_addr).tm_yday = rust_tm.tm_yday;
            (*time_p_addr).tm_isdst = rust_tm.tm_isdst;
            (*time_p_addr).tm_gmtoff = rust_tm.tm_gmtoff as _;
            (*time_p_addr).tm_zone = 0;
        }
        result
    }
}

#[cfg(target_os = "windows")]
pub fn _timegm(mut _ctx: FunctionEnvMut<EmEnv>, _time_ptr: c_int) -> i32 {
    debug!(
        "emscripten::_timegm - UNIMPLEMENTED IN WINDOWS {}",
        _time_ptr
    );
    -1
}

/// emscripten: _strftime
pub fn _strftime(
    ctx: FunctionEnvMut<EmEnv>,
    s_ptr: c_int,
    maxsize: u32,
    format_ptr: c_int,
    tm_ptr: c_int,
) -> i32 {
    debug!(
        "emscripten::_strftime {} {} {} {}",
        s_ptr, maxsize, format_ptr, tm_ptr
    );

    let memory = ctx.data().memory(0);
    #[allow(clippy::cast_ptr_alignment)]
    let s = emscripten_memory_pointer!(memory.view(&ctx), s_ptr) as *mut c_char;
    #[allow(clippy::cast_ptr_alignment)]
    let format = emscripten_memory_pointer!(memory.view(&ctx), format_ptr) as *const c_char;
    #[allow(clippy::cast_ptr_alignment)]
    let tm = emscripten_memory_pointer!(memory.view(&ctx), tm_ptr) as *const guest_tm;

    let format_string = unsafe { std::ffi::CStr::from_ptr(format).to_str().unwrap() };

    debug!("=> format_string: {:?}", format_string);

    let tm = unsafe { &*tm };

    let Ok(rust_date) = time::Date::from_calendar_date(
        tm.tm_year,
        time::Month::try_from(tm.tm_mon as u8).unwrap(),
        tm.tm_mday as u8,
    ) else {
        return 0;
    };
    let Ok(rust_time) = time::Time::from_hms(tm.tm_hour as u8, tm.tm_min as u8, tm.tm_sec as u8)
    else {
        return 0;
    };
    let rust_datetime = time::PrimitiveDateTime::new(rust_date, rust_time);
    let rust_odt =
        rust_datetime.assume_offset(time::UtcOffset::from_whole_seconds(tm.tm_gmtoff).unwrap());

    let result_str = rust_odt
        .format(&time::format_description::parse(format_string).unwrap())
        .unwrap();

    // pad for null?
    let bytes = result_str.chars().count();
    if bytes as u32 > maxsize {
        0
    } else {
        // write output string
        for (i, c) in result_str.chars().enumerate() {
            unsafe { *s.add(i) = c as c_char };
        }
        // null terminate?
        bytes as i32
    }
}

/// emscripten: _strftime_l
pub fn _strftime_l(
    ctx: FunctionEnvMut<EmEnv>,
    s_ptr: c_int,
    maxsize: u32,
    format_ptr: c_int,
    tm_ptr: c_int,
    _last: c_int,
) -> i32 {
    debug!(
        "emscripten::_strftime_l {} {} {} {}",
        s_ptr, maxsize, format_ptr, tm_ptr
    );

    _strftime(ctx, s_ptr, maxsize, format_ptr, tm_ptr)
}
