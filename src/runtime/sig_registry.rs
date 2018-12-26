
use hashbrown::HashMap;
use crate::runtime::{
    types::{Map, SigIndex, FuncSig},
    vm,
};

pub struct SigRegistry {
    sig_set: HashMap<FuncSig, vm::SigId>,
    signatures: Map<vm::SigId, SigIndex>,
}

impl SigRegistry {
    pub fn new() -> Self {
        Self {
            sig_set: HashMap::new(),
            signatures: Map::new(),
        }
    }

    pub fn into_vm_signatures(&self) -> *const vm::SigId {
        self.signatures.as_ptr()
    }

    pub fn register(&mut self, signature: FuncSig) {
        let index = self.sig_set.len();
        let vm_sig_id = self.sig_set.entry(signature).or_insert_with(|| {
            vm::SigId(index as u32)
        });
        self.signatures.push(*vm_sig_id);
    }
}