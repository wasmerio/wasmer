use crate::runtime::types::{ElementType, FuncSig, Table, Type, Val};
use crate::runtime::{Import, Imports, TableBacking};
use crate::webassembly::{ImportObject, ImportValue};
use std::sync::Arc;

extern "C" fn print_i32(num: i32) {
    println!("{}", num);
}

extern "C" fn print() {}

static GLOBAL_I32: i32 = 666;

pub fn spectest_importobject() -> Imports {
    let mut import_object = Imports::new();

    import_object.add(
        "spectest".to_string(),
        "print_i32".to_string(),
        Import::Func(
            print_i32 as _,
            FuncSig {
                params: vec![Type::I32],
                returns: vec![],
            },
        ),
    );

    import_object.add(
        "spectest".to_string(),
        "print".to_string(),
        Import::Func(
            print as _,
            FuncSig {
                params: vec![],
                returns: vec![],
            },
        ),
    );

    import_object.add(
        "spectest".to_string(),
        "global_i32".to_string(),
        Import::Global(Val::I64(GLOBAL_I32 as _)),
    );

    let table = Table {
        ty: ElementType::Anyfunc,
        min: 0,
        max: Some(30),
    };
    import_object.add(
        "spectest".to_string(),
        "table".to_string(),
        Import::Table(Arc::new(TableBacking::new(&table)), table),
    );

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

    /// For a NaN to be canonical, its mantissa bits must all be unset
    fn is_canonical_nan(&self) -> bool {
        let bit_mask: u32 = 0b1____0000_0000____011_1111_1111_1111_1111_1111;
        let masked_value = self.to_bits() ^ bit_mask;
        masked_value == 0xFFFF_FFFF || masked_value == 0x7FFF_FFFF
    }
}

impl NaNCheck for f64 {
    /// The MSB of the mantissa must be set for a NaN to be a quiet NaN.
    fn is_quiet_nan(&self) -> bool {
        let bit_mask = 0b1 << 51; // Used to check if 51st bit is set, which is MSB of the mantissa
        self.is_nan() && (self.to_bits() & bit_mask) == bit_mask
    }

    /// For a NaN to be canonical, its mantissa bits must all be unset
    fn is_canonical_nan(&self) -> bool {
        let bit_mask: u64 =
            0b1____000_0000_0000____0111_1111_1111_1111_1111_1111_1111_1111_1111_1111_1111_1111_1111;
        let masked_value = self.to_bits() ^ bit_mask;
        masked_value == 0x7FFF_FFFF_FFFF_FFFF || masked_value == 0xFFF_FFFF_FFFF_FFFF
    }
}
