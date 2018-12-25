use crate::runtime::{
    vm,
    backing::{LocalBacking, ImportBacking},
    module::{ModuleName, ItemName},
    types::{Val, Memory, Table, Global, FuncSig},
    table::TableBacking,
    memory::LinearMemory,
};
use std::sync::Arc;
use hashbrown::{HashMap, Entry};

pub struct Instance {
    pub vmctx: vm::Ctx,

    backing: LocalBacking,
    imports: ImportBacking,

    pub module: Arc<Module>,
}

impl Instance {
    pub fn new(module: Arc<Module>, imports: &Imports) -> Result<Box<Instance>, String> {
        let mut import_backing = ImportBacking::new(&*module, imports)?;
        let mut backing = LocalBacking::new(&*module, &import_backing);

        let vmctx = vm::Ctx::new(&mut backing, &mut imports);
        
        Ok(Box::new(Instance {
            vmctx,
            backing,
            import_backing,
            module,
        }))
    }
}

#[derive(Debug, PartialEq, Eq)]
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
        self.map.entry(module).or_insert(HashMap::new()).insert(name, import)
    }

    pub fn get(&self, module: ModuleName, name: ItemName) -> Option<&Import> {
        self.map.get().and_then(|m| m.get(name))
    }
}