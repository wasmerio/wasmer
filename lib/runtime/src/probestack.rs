//! This section defines the `PROBESTACK` intrinsic which is used in the
//! implementation of "stack probes" on certain platforms.
//!
//! The purpose of a stack probe is to provide a static guarantee that if a
//! thread has a guard page then a stack overflow is guaranteed to hit that
//! guard page. If a function did not have a stack probe then there's a risk of
//! having a stack frame *larger* than the guard page, so a function call could
//! skip over the guard page entirely and then later hit maybe the heap or
//! another thread, possibly leading to security vulnerabilities such as [The
//! Stack Clash], for example.
//!
//! [The Stack Clash]: https://blog.qualys.com/securitylabs/2017/06/19/the-stack-clash

cfg_if::cfg_if! {
    if #[cfg(any(
        target_arch="aarch64",
        all(
            target_os = "windows",
            target_env = "msvc",
            target_pointer_width = "64"
        )
    ))] {
        extern "C" {
            pub fn __chkstk();
        }
        /// The probestack for Windows when compiled with MSVC.
        /// Also for Aarch64 chipsets.
        pub const PROBESTACK: unsafe extern "C" fn() = __chkstk;
    } else if #[cfg(all(target_os = "windows", target_env = "gnu"))] {
        extern "C" {
            // ___chkstk (note the triple underscore) is implemented in compiler-builtins/src/x86_64.rs
            // by the Rust compiler for the MinGW target
            #[cfg(all(target_os = "windows", target_env = "gnu"))]
            pub fn ___chkstk();
        }
        /// The probestack for Windows when compiled with GNU
        pub const PROBESTACK: unsafe extern "C" fn() = ___chkstk;
    } else {
        extern "C" {
            pub fn __rust_probestack();
        }
        /// The default Rust probestack
        pub static PROBESTACK: unsafe extern "C" fn() = __rust_probestack;
    }
}
