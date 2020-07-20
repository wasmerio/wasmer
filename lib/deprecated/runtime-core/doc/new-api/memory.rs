struct Memory {}

impl Memory {
    fn new(desc: MemoryDescriptor) -> Result<Self, MemoryError>;
    fn descriptor(&self) -> MemoryDescriptor;
    fn grow(&self, delta: Pages) -> Result<Pages, MemoryError>;
    fn size(&self) -> Pages;
    fn view<T: ValueType>(&self) -> MemoryView<T>;
}
