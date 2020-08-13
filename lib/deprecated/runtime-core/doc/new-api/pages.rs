struct Pages(pub u32);

impl Pages {
    fn checked_add(self, rhs: Self) -> Option<Self>;
    fn bytes(self) -> Bytes;
    const fn max_values() -> Self;
}
