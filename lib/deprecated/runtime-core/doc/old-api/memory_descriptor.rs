struct MemoryDescriptor {
    minimum: Pages,
    maximum: Option<Pages>,
    shared: bool,
    memory_type: MemoryType,
}
