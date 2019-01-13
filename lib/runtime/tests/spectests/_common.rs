use wabt::wat2wasm;
use wasmer_clif_backend::CraneliftCompiler;
use wasmer_runtime::import::Imports;

static IMPORT_MODULE: &str = r#"
(module
  (type $t0 (func (param i32)))
  (type $t1 (func))
  (func $print_i32 (export "print_i32") (type $t0) (param $lhs i32))
  (func $print (export "print") (type $t1))
  (table $table (export "table") 10 anyfunc)
  (memory $memory (export "memory") 1)
  (global $global_i32 (export "global_i32") i32 (i32.const 666)))
"#;

pub fn generate_imports() -> Imports {
    let wasm_binary = wat2wasm(IMPORT_MODULE.as_bytes()).expect("WAST not valid or malformed");
    let module = wasmer_runtime::compile(&wasm_binary[..], &CraneliftCompiler::new())
        .expect("WASM can't be compiled");
    let instance = module
        .instantiate(&Imports::new())
        .expect("WASM can't be instantiated");
    let mut imports = Imports::new();
    imports.register("spectest", instance);
    imports
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
        let bit_mask = 0b1 << 51; // Used to check if 52st bit is set, which is MSB of the mantissa
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
