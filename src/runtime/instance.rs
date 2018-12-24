use crate::runtime::{
    vm,
    backing::{LocalBacking, ImportBacking},
};
use std::sync::Arc;

pub struct Instance {
    pub vmctx: vm::Ctx,

    pub finalized_funcs: Box<[*const vm::Func]>,

    pub backing: LocalBacking,
    pub imports: ImportBacking,

    pub module: Arc<Module>,
}

impl Instance {
    pub fn new(module: Arc<Module>) -> Box<Instance> {
        
        Box::new(Instance {
            vmctx,
            finalized_funcs
        })
    }
}