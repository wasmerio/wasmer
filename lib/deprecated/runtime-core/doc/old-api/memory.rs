struct Memory {}

impl Memory {
    fn new(desc: MemoryDescriptor) -> Result<Self, CreationError>;
    fn descriptor(&self) -> MemoryDescriptor;
    fn grow(&self, delta: Pages) -> Result<Pages, GrowError>;
    fn size(&self) -> Pages;
    fn view<T: ValueType>(&self) -> MemoryView<T>;
}
