struct Table {}

impl Table {
    fn new(desc: TableDescriptor) -> Result<Self, CreationError>;
    fn descriptor(&self) -> TableDescriptor;
    fn set<T: StorableInTable>(&self, index: u32, element: T) -> Result<(), TableAccessError>;
    fn size(&self) -> u32;
    fn grow(&self, delta: u32) -> Result<u32, GrowError>;
    fn vm_local_table(&mut self) -> *mut LocalTable;
}
