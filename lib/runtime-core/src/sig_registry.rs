//! A process-global registry for function signatures.
use crate::{
    structures::Map,
    types::{FuncSig, SigIndex},
};
use lazy_static::lazy_static;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

lazy_static! {
    static ref GLOBAL_SIG_REGISTRY: RwLock<GlobalSigRegistry> = {
        let registry = GlobalSigRegistry {
            func_table: HashMap::new(),
            sig_assoc: Map::new(),
        };

        RwLock::new(registry)
    };
}

struct GlobalSigRegistry {
    func_table: HashMap<Arc<FuncSig>, SigIndex>,
    sig_assoc: Map<SigIndex, Arc<FuncSig>>,
}

/// The `SigRegistry` represents a process-global map of function signatures
/// to signature indexes and vice versa (the map goes both ways).
///
/// This exists for two reasons:
/// 1. The `call_indirect` wasm instruction can compare two signature indices
///     to do signature validation very quickly.
/// 2. To intern function signatures, which may be expensive to create.
#[derive(Debug)]
pub struct SigRegistry;

impl SigRegistry {
    /// Map a `FuncSig` to a global `SigIndex`.
    pub fn lookup_sig_index<Sig>(&self, func_sig: Sig) -> SigIndex
    where
        Sig: Into<Arc<FuncSig>>,
    {
        let func_sig = func_sig.into();
        let mut global = (*GLOBAL_SIG_REGISTRY).write();
        let global = &mut *global;

        let func_table = &mut global.func_table;
        let sig_assoc = &mut global.sig_assoc;

        let sig_index = *func_table
            .entry(Arc::clone(&func_sig))
            .or_insert_with(|| sig_assoc.push(func_sig));

        sig_index
    }

    /// Map a global `SigIndex` to an interned `FuncSig`.
    pub fn lookup_signature(&self, sig_index: SigIndex) -> Arc<FuncSig> {
        let global = (*GLOBAL_SIG_REGISTRY).read();
        Arc::clone(&global.sig_assoc[sig_index])
    }

    /// Register a function signature with the global signature registry.
    ///
    /// This will return an interned `FuncSig`.
    pub fn lookup_signature_ref(&self, func_sig: &FuncSig) -> Arc<FuncSig> {
        let mut global = (*GLOBAL_SIG_REGISTRY).write();
        let global = &mut *global;

        let func_table = &mut global.func_table;
        let sig_assoc = &mut global.sig_assoc;

        if func_table.contains_key(func_sig) {
            Arc::clone(&sig_assoc[func_table[func_sig]])
        } else {
            let arc = Arc::new(func_sig.clone());
            func_table.insert(Arc::clone(&arc), sig_assoc.push(Arc::clone(&arc)));
            arc
        }
    }
}
