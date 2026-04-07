// This file contains code from external sources.
// Attributions: https://github.com/wasmerio/wasmer/blob/main/docs/ATTRIBUTIONS.md

//! Implement a registry of function signatures, for fast indirect call
//! signature checking.

use crate::vmcontext::VMSignatureHash;
use std::collections::HashMap;
use std::sync::RwLock;
use wasmer_types::{FunctionType, SignatureHash};

/// WebAssembly requires that the caller and callee signatures in an indirect
/// call must match. To implement this efficiently, keep a registry of all
/// signatures, shared by all instances, so that call sites can just do an
/// index comparison.
#[derive(Debug, Default)]
pub struct SignatureRegistry {
    // This structure is stored in an `Engine` and is intended to be shared
    // across many instances. Ideally instances can themselves be sent across
    // threads, and ideally we can compile across many threads. As a result we
    // use interior mutability here with a lock to avoid having callers to
    // externally synchronize calls to compilation.
    inner: RwLock<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    signature_to_hash: HashMap<FunctionType, VMSignatureHash>,
    hash_to_signature: HashMap<VMSignatureHash, FunctionType>,
}

impl SignatureRegistry {
    /// Create a new `SignatureRegistry`.
    pub fn new() -> Self {
        Default::default()
    }

    /// Register a signature and return its unique hash.
    pub fn register(&self, sig: &FunctionType, hash: SignatureHash) -> VMSignatureHash {
        let mut inner = self.inner.write().unwrap();

        if let Some(sig_hash) = inner.signature_to_hash.get(sig) {
            return *sig_hash;
        }

        let sig_hash = VMSignatureHash::new(hash.0);
        if inner.hash_to_signature.contains_key(&sig_hash) {
            // In theory, two WebAssembly modules (for example, shared libraries) could define different function types
            // that end up with the same hash. We could propagate this information via `Result`, but that would be
            // a breaking change for APIs such as `new_with_env`.
            unreachable!("type signature collision");
        }

        inner.hash_to_signature.insert(sig_hash, sig.clone());
        inner.signature_to_hash.insert(sig.clone(), sig_hash);
        sig_hash
    }

    /// Looks up a registered signature by its hash.
    pub fn lookup_signature(&self, sig_hash: VMSignatureHash) -> Option<FunctionType> {
        self.inner
            .read()
            .unwrap()
            .hash_to_signature
            .get(&sig_hash)
            .cloned()
    }
}
