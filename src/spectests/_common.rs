use crate::webassembly::ImportObject;

extern "C" fn print_i32(num: i32) {
    println!("{}", num);
}

extern "C" fn print() {}

static GLOBAL_I32: i32 = 666;

pub fn spectest_importobject<'a, 'b>() -> ImportObject<&'a str, &'b str> {
    let mut import_object = ImportObject::new();
    import_object.set("spectest", "print_i32", print_i32 as *const u8);
    import_object.set("spectest", "print", print as *const u8);
    import_object.set("spectest", "global_i32", GLOBAL_I32 as *const u8);
    return import_object;
}

/// Bit pattern of an f32 value:
///     1-bit sign + 8-bit mantissa + 23-bit exponent = 32 bits
///
/// Bit pattern of an f64 value:
///     1-bit sign + 11-bit mantissa + 52-bit exponent = 64 bits
///
/// NOTE: On some old platforms (PA-RISC, some MIPS) quiet NaNs (qNaN) have
/// their mantissa MSB unset and set for signaling NaNs (sNaN).
///
/// Links:
///     * https://en.wikipedia.org/wiki/Floating-point_arithmetic
///     * https://github.com/WebAssembly/spec/issues/286
///     * https://en.wikipedia.org/wiki/NaN
///
pub trait NaNCheck {
    fn is_quiet_nan(&self) -> bool;
    fn is_canonical_nan(&self) -> bool;
}

impl NaNCheck for f32 {
    /// The MSB of the mantissa must be set for a NaN to be a quiet NaN.
    fn is_quiet_nan(&self) -> bool {
        let bit_mask = 0b1 << 22; // Used to check if 23rd bit is set, which is MSB of the mantissa
        self.is_nan() && (self.to_bits() & bit_mask) == bit_mask
    }

    /// For a NaN to be canonical, its mantissa bits must all be set,
    /// only the MSB is disregarded. (i.e we don't care if the MSB of the mantissa is set or not)
    fn is_canonical_nan(&self) -> bool {
        let bit_mask = 0b0____1111_1111____011_1111_1111_1111_1111_1111;
        (self.to_bits() & bit_mask) == bit_mask
    }
}

impl NaNCheck for f64 {
    /// The MSB of the mantissa must be set for a NaN to be a quiet NaN.
    fn is_quiet_nan(&self) -> bool {
        let bit_mask = 0b1 << 51; // Used to check if 51st bit is set, which is MSB of the mantissa
        self.is_nan() && (self.to_bits() & bit_mask) == bit_mask
    }

    /// For a NaN to be canonical, its mantissa bits must all be set,
    /// only the MSB is disregarded. (i.e we don't care if the MSB of the mantissa is set or not)
    fn is_canonical_nan(&self) -> bool {
        // 0b0____111_1111_1111____0111_1111_1111_1111 ... 1111
        let bit_mask = 0x7FF7FFFFFFFFFFFF;
        (self.to_bits() & bit_mask) == bit_mask
    }
}
