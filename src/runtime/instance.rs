use crate::runtime::{
    backing::{ImportBacking, LocalBacking},
    memory::LinearMemory,
    module::{ItemName, Module, ModuleName},
    sig_registry::SigRegistry,
    table::TableBacking,
    types::{FuncSig, Memory, Table, Val},
    vm,
};
use hashbrown::HashMap;
use std::sync::Arc;

pub struct Instance {
    pub vmctx: vm::Ctx,

    backing: LocalBacking,
    import_backing: ImportBacking,

    pub module: Arc<Module>,

    pub sig_registry: SigRegistry,
}

impl Instance {
    pub fn new(module: Arc<Module>, imports: &Imports) -> Result<Box<Instance>, String> {
        let mut import_backing = ImportBacking::new(&*module, imports)?;
        let mut backing = LocalBacking::new(&*module, &import_backing);

        let sig_registry = SigRegistry::new();

        let vmctx = vm::Ctx::new(&mut backing, &mut import_backing, &sig_registry);

        Ok(Box::new(Instance {
            vmctx,
            backing,
            import_backing,
            module,
            sig_registry,
        }))
    }
}

#[derive(Debug)]
pub enum Import {
    Func(*const vm::Func, FuncSig),
    Table(Arc<TableBacking>, Table),
    Memory(Arc<LinearMemory>, Memory),
    Global(Val),
}

pub struct Imports {
    map: HashMap<ModuleName, HashMap<ItemName, Import>>,
}

impl Imports {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn add(&mut self, module: ModuleName, name: ItemName, import: Import) {
        self.map
            .entry(module)
            .or_insert(HashMap::new())
            .insert(name, import);
    }

    pub fn get(&self, module: &[u8], name: &[u8]) -> Option<&Import> {
        self.map.get(module).and_then(|m| m.get(name))
    }
}
