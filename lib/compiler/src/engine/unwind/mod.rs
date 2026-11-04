cfg_select! {
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
