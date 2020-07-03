struct Pages(pub u32);

impl Pages {
    fn checked_add(self, rhs: Self) -> Result<Pages, PageError>;
    fn bytes(self) -> Bytes;
}
