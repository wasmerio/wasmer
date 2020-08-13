enum GlobalInit {
    I32Const(i32),
    I64Const(i64),
    F32Const(f32),
    F64Const(f64),
    V128Const(V128),
    GetGlobal(GlobalIndex),
    RefNullConst,
    RefFunc(FunctionIndex),
}

impl GlobalInit {
    fn from_value<T>(value: Value<T>) -> Self;
    fn to_value<T>(&self) -> Value<T>;
}
