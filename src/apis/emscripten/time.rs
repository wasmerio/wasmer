use libc::{timeval, c_int, clock_gettime as libc_clock_gettime, timespec};
use std::time::SystemTime;

use crate::webassembly::Instance;

/// emscripten: _gettimeofday
// pub extern "C" fn _gettimeofday(timeval_ptr_offset: c_int, tz_offset: c_int, instance: &mut Instance) -> c_int {
//     debug!("emscripten::_gettimeofday {}", timeval_ptr_offset);

//     unsafe {
//         let mut timeval_value = *(instance.memory_offset_addr(0, timeval_ptr_offset as _) as *mut timeval);
//         // We skip the timezone for now
//         let mut tz = ptr::null_mut();
//         debug!("emscripten::_gettimeofday(initial) {} {}", (timeval_value).tv_sec, (timeval_value).tv_usec);

//         let returned = gettimeofday(&mut timeval_value, tz);
//         debug!("emscripten::_gettimeofday(filled) {} {}", (timeval_value).tv_sec, (timeval_value).tv_usec);
//         returned
//     }
// }
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
// pub extern "C" fn _clock_gettime(clk_id: clockid_t, tp_offset: c_int, instance: &mut Instance) -> c_int {
//     debug!("emscripten::_clock_gettime {} {}", clk_id, tp_offset);

//     unsafe {
//         let mut tp = instance.memory_offset_addr(0, tp_offset as _) as *mut timespec;
//         let returned = clock_gettime(clk_id, tp);
//         debug!("emscripten::clock_gettime(filled) {} {}", (*tp).tv_sec, (*tp).tv_nsec);
//         returned
//     }
// }

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