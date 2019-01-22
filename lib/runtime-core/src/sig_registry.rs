use crate::{
    structures::Map,
    types::{FuncSig, SigIndex},
};
use hashbrown::HashMap;

#[derive(Debug)]
pub struct SigRegistry {
    func_table: HashMap<FuncSig, SigIndex>,
    sig_assoc: Map<SigIndex, FuncSig>,
    duplicated_sig_assoc: Map<SigIndex, SigIndex>,
}

impl SigRegistry {
    pub fn new() -> Self {
        Self {
            func_table: HashMap::new(),
            sig_assoc: Map::new(),
            duplicated_sig_assoc: Map::new(),
        }
    }

    pub fn register(&mut self, func_sig: FuncSig) -> SigIndex {
        // self.sig_assoc.push(func_sig)
        let func_table = &mut self.func_table;
        let sig_assoc = &mut self.sig_assoc;
        let sig_index = *func_table
            .entry(func_sig.clone())
            .or_insert_with(|| sig_assoc.push(func_sig));
        self.duplicated_sig_assoc.push(sig_index);
        sig_index
    }

    pub fn lookup_deduplicated_sigindex(&self, sig_index: SigIndex) -> SigIndex {
        self.duplicated_sig_assoc[sig_index]
    }

    pub fn lookup_func_sig(&self, sig_index: SigIndex) -> &FuncSig {
        &self.sig_assoc[sig_index]
    }
}
