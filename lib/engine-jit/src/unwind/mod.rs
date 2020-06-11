cfg_if::cfg_if! {
    if #[cfg(all(windows, target_arch = "x86_64"))] {
        mod windows_x64;
        pub use self::windows_x64::*;
    } else if #[cfg(all(windows, target_arch = "x86"))] {
        mod windows_x32;
        pub use self::windows_x32::*;
    } else if #[cfg(unix)] {
        mod systemv;
        pub use self::systemv::*;
    } else {
        compile_error!("unsupported target platform for unwind");
    }
}
