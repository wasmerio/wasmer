// Part of the logic, here, is borrowed as-is from rust's stdlib.

mod dwarf;

cfg_if::cfg_if! {
    if #[cfg(any(target_env = "msvc", target_family = "wasm"))] {
        // We have yet to figure this out.
        fn wasmer_eh_personality() {
            core::intrinsics::abort()
        }
    } else if #[cfg(any(
        all(target_family = "windows", target_env = "gnu"),
        target_family = "unix",
    ))] {
        // gcc-like eh-personality mechanisms.
        mod gcc;
        pub use gcc::*;
    } else {
        // Targets that don't support unwinding.
        // - os=none ("bare metal" targets)
        // - os=uefi
        // - os=espidf
        // - os=hermit
        // - nvptx64-nvidia-cuda
        // - arch=avr
    }
}
