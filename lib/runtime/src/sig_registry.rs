use crate::{
    types::{FuncSig, Map, MapIndex, SigIndex},
    vm,
};
// use hashbrown::HashMap;

pub struct SigRegistry {
    // func_table: HashMap<FuncSig, SigIndex>,
    sig_assoc: Map<SigIndex, FuncSig>,
}

impl SigRegistry {
    pub fn new() -> Self {
        Self {
            // func_table: HashMap::new(),
            sig_assoc: Map::new(),
        }
    }

    pub fn register(&mut self, func_sig: FuncSig) -> SigIndex {
        self.sig_assoc.push(func_sig)
        // let func_table = &mut self.func_table;
        // let sig_assoc = &mut self.sig_assoc;
        // *func_table
        //     .entry(func_sig.clone())
        //     .or_insert_with(|| sig_assoc.push(func_sig))
    }

    pub fn lookup_func_sig(&self, sig_index: SigIndex) -> &FuncSig {
        &self.sig_assoc[sig_index]
    }

    pub(crate) fn into_vm_sigid(&self) -> Box<[vm::SigId]> {
        let v: Vec<_> = self
            .sig_assoc
            .iter()
            .map(|(sig_index, _)| vm::SigId(sig_index.index() as u32))
            .collect();
        v.into_boxed_slice()
    }
}
