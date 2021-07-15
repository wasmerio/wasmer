use std::convert::TryFrom;
use wasmer_api::Mutability;

#[allow(non_camel_case_types)]
pub type wasm_mutability_t = u8;

#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum wasm_mutability_enum {
    WASM_CONST = 0,
    WASM_VAR,
}

impl wasm_mutability_enum {
    #[allow(dead_code)]
    fn is_mutable(self) -> bool {
        self == Self::WASM_VAR
    }
}

impl TryFrom<wasm_mutability_t> for wasm_mutability_enum {
    type Error = &'static str;

    fn try_from(item: wasm_mutability_t) -> Result<Self, Self::Error> {
        Ok(match item {
            0 => wasm_mutability_enum::WASM_CONST,
            1 => wasm_mutability_enum::WASM_VAR,
            _ => return Err("wasm_mutability_t value out of bounds"),
        })
    }
}

impl From<wasm_mutability_enum> for wasm_mutability_t {
    fn from(other: wasm_mutability_enum) -> Self {
        other as wasm_mutability_t
    }
}

impl From<wasm_mutability_enum> for Mutability {
    fn from(other: wasm_mutability_enum) -> Self {
        match other {
            wasm_mutability_enum::WASM_CONST => Mutability::Const,
            wasm_mutability_enum::WASM_VAR => Mutability::Var,
        }
    }
}

impl From<Mutability> for wasm_mutability_enum {
    fn from(other: Mutability) -> Self {
        match other {
            Mutability::Const => wasm_mutability_enum::WASM_CONST,
            Mutability::Var => wasm_mutability_enum::WASM_VAR,
        }
    }
}
