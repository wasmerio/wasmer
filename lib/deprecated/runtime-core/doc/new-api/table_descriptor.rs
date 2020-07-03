struct TableDescriptor {
    ty: Type,
    minimum: u32,
    maximum: Option<u32>,
}

impl TableDescriptor {
    fn new(ty: Type, minimum: u32, maximum: Option<u32>) -> Self;
}
