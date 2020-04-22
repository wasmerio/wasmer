use wasmer_jit::Resolver;
use std::iter::FromIterator;

pub struct IndexResolver {
    externs: Vec<Extern>,
}
impl Resolver for IndexResolver {
    fn resolve(&self, index: u32, _module: &str, _name: &str) -> Option<Export> {
        self.externs.get(index as usize).map(|extern_| extern_.to_export())
    }
}

impl FromIterator<Extern> for IndexResolver {
    fn from_iter<I: IntoIterator<Item = Extern>>(iter: I) -> Self {
        let mut externs = Vec::new();
        for extern_ in iter {
            externs.push(extern_);
        }
        IndexResolver { externs }
    }
}
