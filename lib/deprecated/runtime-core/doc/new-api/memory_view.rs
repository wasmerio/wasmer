struct MemoryView {}

impl MemoryView {
    fn atomically(&self) -> MemoryView<'a, T::Output, Atomically>;
}
