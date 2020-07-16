struct MemoryDescriptor {
    minimum: Pages,
    maximum: Option<Pages>,
    shared: bool,
}
