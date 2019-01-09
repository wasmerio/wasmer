// F32
#[inline]
pub extern "C" fn ceilf32(x: f32) -> f32 {
    x.ceil()
}

#[inline]
pub extern "C" fn floorf32(x: f32) -> f32 {
    x.floor()
}

#[inline]
pub extern "C" fn truncf32(x: f32) -> f32 {
    x.trunc()
}

#[inline]
pub extern "C" fn nearbyintf32(x: f32) -> f32 {
    x.round()
}

// F64
#[inline]
pub extern "C" fn ceilf64(x: f64) -> f64 {
    x.ceil()
}

#[inline]
pub extern "C" fn floorf64(x: f64) -> f64 {
    x.floor()
}

#[inline]
pub extern "C" fn truncf64(x: f64) -> f64 {
    x.trunc()
}

#[inline]
pub extern "C" fn nearbyintf64(x: f64) -> f64 {
    x.round()
}

/// A declaration for the stack probe function in Rust's standard library, for
/// catching callstack overflow.
extern "C" {
    pub fn __rust_probestack();
}
