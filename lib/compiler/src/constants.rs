// Constants for the bounds of truncation operations. These are the least or
// greatest exact floats in either f32 or f64 representation
// greater-than-or-equal-to (for least) or less-than-or-equal-to (for greatest)
// the i32 or i64 or u32 or u64 min (for least) or max (for greatest), when
// rounding towards zero.

/// Least Exact Float (32 bits) greater-than-or-equal-to i32::MIN when rounding towards zero.
pub const LEF32_GEQ_I32_MIN: u64 = i32::MIN as u64;
/// Greatest Exact Float (32 bits) less-than-or-equal-to i32::MAX when rounding towards zero.
pub const GEF32_LEQ_I32_MAX: u64 = 2147483520; // bits as f32: 0x4eff_ffff
/// Least Exact Float (64 bits) greater-than-or-equal-to i32::MIN when rounding towards zero.
pub const LEF64_GEQ_I32_MIN: u64 = i32::MIN as u64;
/// Greatest Exact Float (64 bits) less-than-or-equal-to i32::MAX when rounding towards zero.
pub const GEF64_LEQ_I32_MAX: u64 = i32::MAX as u64;
/// Least Exact Float (32 bits) greater-than-or-equal-to u32::MIN when rounding towards zero.
pub const LEF32_GEQ_U32_MIN: u64 = u32::MIN as u64;
/// Greatest Exact Float (32 bits) less-than-or-equal-to u32::MAX when rounding towards zero.
pub const GEF32_LEQ_U32_MAX: u64 = 4294967040; // bits as f32: 0x4f7f_ffff
/// Least Exact Float (64 bits) greater-than-or-equal-to u32::MIN when rounding towards zero.
pub const LEF64_GEQ_U32_MIN: u64 = u32::MIN as u64;
/// Greatest Exact Float (64 bits) less-than-or-equal-to u32::MAX when rounding towards zero.
pub const GEF64_LEQ_U32_MAX: u64 = 4294967295; // bits as f64: 0x41ef_ffff_ffff_ffff
/// Least Exact Float (32 bits) greater-than-or-equal-to i64::MIN when rounding towards zero.
pub const LEF32_GEQ_I64_MIN: u64 = i64::MIN as u64;
/// Greatest Exact Float (32 bits) less-than-or-equal-to i64::MAX when rounding towards zero.
pub const GEF32_LEQ_I64_MAX: u64 = 9223371487098961920; // bits as f32: 0x5eff_ffff
/// Least Exact Float (64 bits) greater-than-or-equal-to i64::MIN when rounding towards zero.
pub const LEF64_GEQ_I64_MIN: u64 = i64::MIN as u64;
/// Greatest Exact Float (64 bits) less-than-or-equal-to i64::MAX when rounding towards zero.
pub const GEF64_LEQ_I64_MAX: u64 = 9223372036854774784; // bits as f64: 0x43df_ffff_ffff_ffff
/// Least Exact Float (32 bits) greater-than-or-equal-to u64::MIN when rounding towards zero.
pub const LEF32_GEQ_U64_MIN: u64 = u64::MIN;
/// Greatest Exact Float (32 bits) less-than-or-equal-to u64::MAX when rounding towards zero.
pub const GEF32_LEQ_U64_MAX: u64 = 18446742974197923840; // bits as f32: 0x5f7f_ffff
/// Least Exact Float (64 bits) greater-than-or-equal-to u64::MIN when rounding towards zero.
pub const LEF64_GEQ_U64_MIN: u64 = u64::MIN;
/// Greatest Exact Float (64 bits) less-than-or-equal-to u64::MAX when rounding towards zero.
pub const GEF64_LEQ_U64_MAX: u64 = 18446744073709549568; // bits as f64: 0x43ef_ffff_ffff_ffff

/// Canonical NaN value for f32 type
pub const CANONICAL_NAN_F32: u32 = 0x7fc00000;
/// Canonical NaN value for f64 type
pub const CANONICAL_NAN_F64: u64 = 0x7ff8000000000000;
