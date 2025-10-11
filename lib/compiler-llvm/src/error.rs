macro_rules! err_nt {
    ($e: expr_2021) => {
        $e.map_err(|v| CompileError::Codegen(v.to_string()))
    };
}

macro_rules! err {
    ($e: expr_2021) => {
        $e.map_err(|v| CompileError::Codegen(v.to_string()))?
    };
}

pub(crate) use err;
pub(crate) use err_nt;
