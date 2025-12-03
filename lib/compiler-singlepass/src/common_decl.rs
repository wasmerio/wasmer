#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Size {
    S8,
    S16,
    S32,
    S64,
}

impl Size {
    #[allow(dead_code)]
    pub fn bits(&self) -> u32 {
        8 * self.bytes()
    }

    pub fn bytes(&self) -> u32 {
        match self {
            Size::S8 => 1,
            Size::S16 => 2,
            Size::S32 => 4,
            Size::S64 => 8,
        }
    }
}
