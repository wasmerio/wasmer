// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/master/ATTRIBUTIONS.md

use std::boxed::Box;
use wasmer_types::entity::PrimaryMap;
use wasmer_types::{SignatureIndex, WasmResult};

/// Map of signatures to a function's parameter and return types.
pub(crate) type WasmTypes =
    PrimaryMap<SignatureIndex, (Box<[wasmparser::ValType]>, Box<[wasmparser::ValType]>)>;

/// Contains information decoded from the Wasm module that must be referenced
/// during each Wasm function's translation.
///
/// This is only for data that is maintained by `wasmer-compiler` itself, as
/// opposed to being maintained by the embedder. Data that is maintained by the
/// embedder is represented with `ModuleEnvironment`.
#[derive(Debug)]
pub struct ModuleTranslationState {
    /// A map containing a Wasm module's original, raw signatures.
    ///
    /// This is used for translating multi-value Wasm blocks inside functions,
    /// which are encoded to refer to their type signature via index.
    pub(crate) wasm_types: WasmTypes,
}

impl ModuleTranslationState {
    /// Creates a new empty ModuleTranslationState.
    pub fn new() -> Self {
        Self {
            wasm_types: PrimaryMap::new(),
        }
    }

    /// Get the parameter and result types for the given Wasm blocktype.
    pub fn blocktype_params_results<'a>(
        &'a self,
        ty_or_ft: &'a wasmparser::BlockType,
    ) -> WasmResult<(&'a [wasmparser::ValType], SingleOrMultiValue<'a>)> {
        Ok(match ty_or_ft {
            wasmparser::BlockType::Type(ty) => (&[], SingleOrMultiValue::Single(ty)),
            wasmparser::BlockType::FuncType(ty_index) => {
                let sig_idx = SignatureIndex::from_u32(*ty_index);
                let (ref params, ref results) = self.wasm_types[sig_idx];
                (params, SingleOrMultiValue::Multi(results.as_ref()))
            }
            wasmparser::BlockType::Empty => (&[], SingleOrMultiValue::Multi(&[])),
        })
    }
}

/// A helper enum for representing either a single or multiple values.
pub enum SingleOrMultiValue<'a> {
    /// A single value.
    Single(&'a wasmparser::ValType),
    /// Multiple values.
    Multi(&'a [wasmparser::ValType]),
}

impl<'a> SingleOrMultiValue<'a> {
    /// True if empty.
    pub fn is_empty(&self) -> bool {
        match self {
            SingleOrMultiValue::Single(_) => false,
            SingleOrMultiValue::Multi(values) => values.is_empty(),
        }
    }

    /// Count of values.
    pub fn len(&self) -> usize {
        match self {
            SingleOrMultiValue::Single(_) => 1,
            SingleOrMultiValue::Multi(values) => values.len(),
        }
    }

    /// Iterate ofer the value types.
    pub fn iter(&self) -> SingleOrMultiValueIterator<'_> {
        match self {
            SingleOrMultiValue::Single(v) => SingleOrMultiValueIterator::Single(v),
            SingleOrMultiValue::Multi(items) => SingleOrMultiValueIterator::Multi {
                index: 0,
                values: items,
            },
        }
    }
}

pub enum SingleOrMultiValueIterator<'a> {
    Done,
    Single(&'a wasmparser::ValType),
    Multi {
        index: usize,
        values: &'a [wasmparser::ValType],
    },
}

impl<'a> Iterator for SingleOrMultiValueIterator<'a> {
    type Item = &'a wasmparser::ValType;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            SingleOrMultiValueIterator::Done => None,
            SingleOrMultiValueIterator::Single(v) => {
                let v = *v;
                *self = SingleOrMultiValueIterator::Done;
                Some(v)
            }
            SingleOrMultiValueIterator::Multi { index, values } => {
                if let Some(x) = values.get(*index) {
                    *index += 1;
                    Some(x)
                } else {
                    *self = SingleOrMultiValueIterator::Done;
                    None
                }
            }
        }
    }
}

impl<'a> PartialEq<[wasmparser::ValType]> for SingleOrMultiValue<'a> {
    fn eq(&self, other: &[wasmparser::ValType]) -> bool {
        match self {
            SingleOrMultiValue::Single(ty) => other.len() == 1 && &other[0] == *ty,
            SingleOrMultiValue::Multi(tys) => *tys == other,
        }
    }
}

impl<'a> PartialEq<SingleOrMultiValue<'a>> for &'a [wasmparser::ValType] {
    fn eq(&self, other: &SingleOrMultiValue<'a>) -> bool {
        match other {
            SingleOrMultiValue::Single(ty) => self.len() == 1 && &self[0] == *ty,
            SingleOrMultiValue::Multi(tys) => tys == self,
        }
    }
}
