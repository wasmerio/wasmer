#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Shard {
    #[default]
    Singleton,
    ById(u64),
}
