struct FuncSig {}

impl FuncSig {
    fn new<Params, Returns>(params: Params, returns: Returns) -> Self;
    fn params(&self) -> &[Type];
    fn results(&self) -> &[Type];
}
