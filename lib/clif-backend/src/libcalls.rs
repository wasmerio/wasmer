use std::{f32, f64};

// F32
pub extern "C" fn ceilf32(x: f32) -> f32 {
    x.ceil()
}

pub extern "C" fn floorf32(x: f32) -> f32 {
    x.floor()
}

pub extern "C" fn truncf32(x: f32) -> f32 {
    x.trunc()
}

/// `f32.round()` doesn't have the correct behavior. Ideally, we'd use
/// "https://doc.rust-lang.org/std/intrinsics/fn.nearbyintf32.html" for this,
/// but support for stable compilers is necessary, so we must implement
/// this ourselves.
/// This is ported from "https://github.com/rust-lang/rust/issues/55107#issuecomment-431247454"
pub extern "C" fn nearbyintf32(x: f32) -> f32 {
    #[inline]
    fn copysign(x: f32, y: f32) -> f32 {
        let bitmask = y.to_bits() & (1 << 31);
        f32::from_bits(x.to_bits() | bitmask)
    }

    if x.is_nan() {
        f32::from_bits(x.to_bits() | (1 << 22))
    } else {
        let k = f32::EPSILON.recip();
        let a = x.abs();
        if a < k {
            copysign((a + k) - k, x)
        } else {
            x
        }
    }
}

// F64
pub extern "C" fn ceilf64(x: f64) -> f64 {
    x.ceil()
}

pub extern "C" fn floorf64(x: f64) -> f64 {
    x.floor()
}

pub extern "C" fn truncf64(x: f64) -> f64 {
    x.trunc()
}

/// `f64.round()` doesn't have the correct behavior. Ideally, we'd use
/// "https://doc.rust-lang.org/std/intrinsics/fn.nearbyintf64.html" for this,
/// but support for stable compilers is necessary, so we must implement
/// this ourselves.
/// This is ported from "https://github.com/rust-lang/rust/issues/55007#issuecomment-431247454"
pub extern "C" fn nearbyintf64(x: f64) -> f64 {
    #[inline]
    fn copysign(x: f64, y: f64) -> f64 {
        let bitmask = y.to_bits() & (1 << 63);
        f64::from_bits(x.to_bits() | bitmask)
    }

    if x.is_nan() {
        f64::from_bits(x.to_bits() | (1 << 51))
    } else {
        let k = f64::EPSILON.recip();
        let a = x.abs();
        if a < k {
            copysign((a + k) - k, x)
        } else {
            x
        }
    }
}

// FIXME: Is there a replacement on AArch64?
#[cfg(all(
    any(target_os = "freebsd", target_os = "linux"),
    target_arch = "aarch64"
))]
#[no_mangle]
pub extern "C" fn __rust_probestack() {}
