#[allow(dead_code)]
#[derive(Clone, PartialEq)]
pub enum Value {
    I8(i8),
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
}

pub trait MaybeImmediate {
    fn imm_value(&self) -> Option<Value>;
    fn is_imm(&self) -> bool {
        self.imm_value().is_some()
    }
}
