//! Preparation of a supported dynamic main before the existing exec instrumenter/compiler.
use crate::{Initialization, Module, PlanError};
use std::borrow::Cow;
use wasmparser::{Parser, Payload};

pub const SOURCE_SECTION: &str = "wasixv2.dylink.source";

#[derive(Default)]
struct Profile {
    dynamic: bool,
    native: bool,
    legacy: bool,
    prepared: bool,
}

fn profile(bytes: &[u8]) -> Result<Profile, PlanError> {
    let mut profile = Profile::default();
    for payload in Parser::new(0).parse_all(bytes) {
        match payload? {
            Payload::CustomSection(section) => {
                if section.name() == SOURCE_SECTION {
                    profile.prepared = true;
                }
                profile.dynamic |= section.name() == "dylink.0";
            }
            Payload::ImportSection(section) => {
                for import in section.into_imports() {
                    let module = import?.module;
                    profile.native |= module == "wasixv2";
                    profile.legacy |= matches!(
                        module,
                        "wasix_32v1" | "wasi_snapshot_preview1" | "wasi_unstable" | "wasi"
                    );
                }
            }
            _ => {}
        }
    }
    Ok(profile)
}

/// Whether source selects raw native or wasm32 v1 dynamic-main preparation. Static
/// and already-prepared inputs retain their existing compilation path. This classifies
/// source only; it does not validate prepared source markers or grant runtime admission.
pub fn needs_preparation(bytes: &[u8]) -> Result<bool, PlanError> {
    let profile = profile(bytes)?;
    Ok(profile.dynamic && (profile.native || profile.legacy) && !profile.prepared)
}

/// Static images pass through byte-for-byte. A supported dynamic main is rewritten before
/// compilation and carries its validated original source for the store-local ABI binder.
/// Embedders with custom compilers call this before exec's normal preemption instrumenter.
pub fn prepare_image(bytes: &[u8]) -> Result<Vec<u8>, PlanError> {
    let profile = profile(bytes)?;
    if profile.prepared {
        return Err(PlanError::Metadata("reserved source section"));
    }
    if !profile.dynamic || !(profile.native || profile.legacy) {
        return Ok(bytes.to_vec());
    }
    let metadata = Module::parse(bytes)?;
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
