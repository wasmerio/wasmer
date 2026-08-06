cfg_select! {
    all(windows, target_arch = "x86_64") => {
        mod windows_x64;
        pub use self::windows_x64::*;
    }
    unix => {
        mod systemv;
        pub use self::systemv::*;
    }
    _ => {
        // Otherwise, we provide a dummy fallback without unwinding
        mod dummy;
        pub use self::dummy::DummyUnwindRegistry as UnwindRegistry;
    }
}
