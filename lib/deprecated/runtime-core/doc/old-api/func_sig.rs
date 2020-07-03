struct FuncSig {}

impl FuncSig {
    fn new<Params, Returns>(params: Params, returns: Returns) -> Self;
    fn params(&self) -> &[Type];
    fn returns(&self) -> &[Type];
    fn check_param_value_types(&self, params: &[Value]) -> bool;
}
