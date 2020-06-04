use std::iter::FromIterator;
use wasmer_engine::Resolver;

use crate::exports::Exportable;
use crate::Extern;
use wasmer_runtime::Export;

#[derive(Clone)]
pub struct OrderedResolver {
    externs: Vec<Extern>,
}
impl Resolver for OrderedResolver {
    fn resolve(&self, index: u32, _module: &str, _name: &str) -> Option<Export> {
        self.externs
            .get(index as usize)
            .map(|extern_| extern_.to_export())
    }
}

impl FromIterator<Extern> for OrderedResolver {
    fn from_iter<I: IntoIterator<Item = Extern>>(iter: I) -> Self {
        let mut externs = Vec::new();
        for extern_ in iter {
            externs.push(extern_);
        }
        OrderedResolver { externs }
    }
}
