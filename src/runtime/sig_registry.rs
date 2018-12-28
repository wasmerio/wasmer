use crate::runtime::{
    module::Module,
    types::{FuncSig, Map, SigIndex},
    vm,
};
use hashbrown::HashMap;

pub struct SigRegistry {
    sig_set: HashMap<FuncSig, vm::SigId>,
    signatures: Map<SigIndex, vm::SigId>,
}

impl SigRegistry {
    pub fn new(module: &Module) -> Self {
        let mut registry = Self {
            sig_set: HashMap::new(),
            signatures: Map::new(),
        };

        for (_, &sig_index) in &module.signature_assoc {
            let func_sig = module.signatures[sig_index].clone();
            let new_sig_index = registry.register(func_sig);
            assert_eq!(sig_index, new_sig_index);
        }

        registry
    }

    pub fn into_vm_signatures(&self) -> *const vm::SigId {
        self.signatures.as_ptr()
    }

    pub fn get_vm_id(&self, sig_index: SigIndex) -> vm::SigId {
        self.signatures[sig_index]
    }

    fn register(&mut self, signature: FuncSig) -> SigIndex {
        let index = self.sig_set.len();
        let vm_sig_id = *self
            .sig_set
            .entry(signature)
            .or_insert_with(|| vm::SigId(index as u32));
        self.signatures.push(vm_sig_id)
    }
}
