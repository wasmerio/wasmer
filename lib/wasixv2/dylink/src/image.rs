//! Preparation of a native dynamic main before the existing exec instrumenter/compiler.
use crate::{Initialization, Module, PlanError};
use std::borrow::Cow;
use wasmparser::{Parser, Payload};

pub const SOURCE_SECTION: &str = "wasixv2.dylink.source";

/// Static/v1 images pass through byte-for-byte. A native dynamic main is rewritten before
/// compilation and carries its validated original source for the store-local ABI binder.
/// Embedders with custom compilers call this before exec's normal preemption instrumenter.
pub fn prepare_image(bytes: &[u8]) -> Result<Vec<u8>, PlanError> {
    let mut dynamic = false;
    let mut native = false;
    for payload in Parser::new(0).parse_all(bytes) {
        match payload? {
            Payload::CustomSection(section) => {
                if section.name() == SOURCE_SECTION {
                    return Err(PlanError::Metadata("reserved source section"));
                }
                dynamic |= section.name() == "dylink.0";
            }
            Payload::ImportSection(section) => {
                for import in section.into_imports() {
                    native |= import?.module == "wasixv2";
                }
            }
            _ => {}
        }
    }
    if !dynamic || !native {
        return Ok(bytes.to_vec());
    }
    let metadata = Module::parse(bytes)?;
    if !metadata.needed().is_empty() {
        return Err(PlanError::Unsupported("dynamic-main startup dependencies"));
    }
    if metadata
        .exports()
        .keys()
        .any(|name| name.starts_with("asyncify_"))
    {
        return Err(PlanError::Unsupported("asyncify dynamic main"));
    }
    let init = Initialization::parse(bytes)?;
    let mut rewritten = init.instantiate_bytes()?;
    let section = wasm_encoder::CustomSection {
        name: Cow::Borrowed(SOURCE_SECTION),
        data: Cow::Borrowed(bytes),
    };
    use wasm_encoder::{Encode, Section};
    rewritten.push(section.id());
    section.encode(&mut rewritten);
    Ok(rewritten)
}
