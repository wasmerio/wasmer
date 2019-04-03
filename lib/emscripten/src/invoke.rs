use wasmer_runtime_core::backend::CompilerConfig;
use wasmer_runtime_core::{compile_with_config, Module};

struct Sig {
    pub args: Vec<InvokeArgType>,
    pub rets: Vec<InvokeArgType>,
}

pub fn create_invoke_module(invoke_function_names: &Vec<String>) -> Module {
    let wat = create_wat(invoke_function_names);
    let wasm_binary = wabt::wat2wasm(wat.as_bytes()).unwrap();
    let compiler = wasmer_clif_backend::CraneliftCompiler::new();
    let compiler_config = CompilerConfig { symbol_map: None };
    let module = compile_with_config(&wasm_binary[..], &compiler, compiler_config);
    module.unwrap()
}

fn create_wat(invoke_function_names: &Vec<String>) -> String {
    let types_and_funcs_wat = invoke_function_names
        .iter()
        .map(|s| &**s) // &String -> &str
        .map(invoke_name_to_signature)
        .filter_map(|r| r.ok())
        .map(|(func_name, sig)| {
            let type_wat = sig_to_type_wat(func_name, &sig);
            let func_wat = sig_to_func_wat(func_name, &sig);
            format!("{}\n{}", type_wat, func_wat)
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "(module (table {} anyfunc)\n {})",
        invoke_function_names.len(),
        types_and_funcs_wat
    )
}

fn invoke_name_to_signature(invoke_name: &str) -> Result<(&str, Sig), InvokeError> {
    let mut args: Vec<InvokeArgType> = vec![];
    let mut rets: Vec<InvokeArgType> = vec![];
    let chars = invoke_name.chars();
    // skip 'invoke_'
    let mut chars = chars.skip(7);
    // 'if next is a 'v' then this does not return
    match chars.next() {
        Some('v') => {}
        Some(c) => match InvokeArgType::from_char(c) {
            Ok(i) => rets.push(i),
            Err(_) => return Err(InvokeError::InvalidInvokeFunctionParameterCharacter),
        },
        None => {}
    };
    // continue iterating
    for c in chars {
        match InvokeArgType::from_char(c) {
            Ok(i) => args.push(i),
            Err(_) => return Err(InvokeError::InvalidInvokeFunctionParameterCharacter),
        }
    }

    let sig = Sig { args, rets };
    Ok((invoke_name, sig))
}

fn sig_to_type_wat(func_name: &str, sig: &Sig) -> String {
    let x = sig.args.iter().map(|i| i.to_type_str()).collect::<Vec<_>>();
    let params_string = match &x[..] {
        [] => "".to_string(),
        _args => format!(" (param {})", x.join(" ")),
    };
    let result_string = match sig.rets.get(0) {
        Some(ret) => format!(" (result {})", ret.to_type_str()),
        None => "".to_string(),
    };
    format!(
        "(type $emscripten_{} (func{}{}))",
        func_name, params_string, result_string
    )
}

fn sig_to_func_wat(func_name: &str, sig: &Sig) -> String {
    let x = sig
        .args
        .iter()
        .zip(1..)
        .map(|(a, i)| format!("(param $p{} {})", i, a.to_type_str()))
        .collect::<Vec<_>>();
    let params_string = match &x[..] {
        [] => "".to_string(),
        params => format!(" {}", params.join(" ")),
    };
    let result_string = match sig.rets.get(0) {
        Some(ret) => format!(" (result {})", ret.to_type_str()),
        None => "".to_string(),
    };
    let get_locals_vec = sig
        .args
        .iter()
        .zip(1..)
        .map(|(_, i)| format!("get_local $p{}", i))
        .collect::<Vec<_>>();
    //    get_locals_vec.reverse();
    let mut get_locals_string = get_locals_vec.join("\n");
    if get_locals_string.len() > 0 {
        get_locals_string = get_locals_string + "\n";
    }
    format!("(func (export \"{}\") (param $p0 i32){}{}\n{}get_local $p0\ncall_indirect (type $emscripten_{}))", func_name, params_string, result_string, get_locals_string, func_name)
}

#[derive(Debug)]
enum InvokeError {
    InvalidInvokeFunctionParameterCharacter,
}

#[derive(Debug, PartialEq)]
enum InvokeArgType {
    D, // f64
    I, // i32
    J, // u32
}

impl InvokeArgType {
    pub fn from_char(c: char) -> Result<InvokeArgType, InvokeError> {
        match c {
            'd' => Ok(InvokeArgType::D),
            'i' => Ok(InvokeArgType::I),
            'j' => Ok(InvokeArgType::J),
            _ => Err(InvokeError::InvalidInvokeFunctionParameterCharacter),
        }
    }

    pub fn to_type_str(&self) -> &str {
        match self {
            InvokeArgType::D => "f64",
            InvokeArgType::J => "u32",
            InvokeArgType::I => "i32",
        }
    }
}

#[cfg(test)]
mod test {
    use crate::invoke::{
        create_invoke_module, invoke_name_to_signature, sig_to_func_wat, sig_to_type_wat,
        InvokeArgType, Sig,
    };
    use crate::{generate_emscripten_env, EmscriptenGlobals};
    use wasmer_runtime_core::import::{ImportObject, LikeNamespace};
    use wasmer_runtime_core::Instance;

    #[test]
    fn create_type_wat_for_invoke_ii() {
        let expected_wat = "(type $emscripten_invoke_ii (func (param i32) (result i32)))";
        let sig = Sig {
            args: vec![InvokeArgType::I],
            rets: vec![InvokeArgType::I],
        };
        let func_name = "invoke_ii";
        let actual_wat = sig_to_type_wat(func_name, &sig);
        assert_eq!(expected_wat, actual_wat);
    }
    #[test]
    fn create_type_wat_for_invoke_i() {
        let expected_wat = "(type $emscripten_invoke_i (func (result i32)))";
        let sig = Sig {
            args: vec![],
            rets: vec![InvokeArgType::I],
        };
        let func_name = "invoke_i";
        let actual_wat = sig_to_type_wat(func_name, &sig);
        assert_eq!(expected_wat, actual_wat);
    }
    #[test]
    fn create_type_wat_for_invoke_vi() {
        let expected_wat = "(type $emscripten_invoke_vi (func (param i32)))";
        let sig = Sig {
            args: vec![InvokeArgType::I],
            rets: vec![],
        };
        let func_name = "invoke_vi";
        let actual_wat = sig_to_type_wat(func_name, &sig);
        assert_eq!(expected_wat, actual_wat);
    }

    #[test]
    fn create_func_wat_for_invoke_ii() {
        let expected_wat =
            r#"(func (export "invoke_ii") (param $p0 i32) (param $p1 i32) (result i32)
get_local $p1
get_local $p0
call_indirect (type $emscripten_invoke_ii))"#;
        let sig = Sig {
            args: vec![InvokeArgType::I],
            rets: vec![InvokeArgType::I],
        };
        let func_name = "invoke_ii";
        let actual_wat = sig_to_func_wat(func_name, &sig);
        assert_eq!(expected_wat, actual_wat);
    }

    #[test]
    fn create_func_wat_for_invoke_iiii() {
        let expected_wat = r#"(func (export "invoke_iiii") (param $p0 i32) (param $p1 i32) (param $p2 i32) (param $p3 i32) (result i32)
get_local $p1
get_local $p2
get_local $p3
get_local $p0
call_indirect (type $emscripten_invoke_iiii))"#;
        let sig = Sig {
            args: vec![InvokeArgType::I, InvokeArgType::I, InvokeArgType::I],
            rets: vec![InvokeArgType::I],
        };
        let func_name = "invoke_iiii";
        let actual_wat = sig_to_func_wat(func_name, &sig);
        assert_eq!(expected_wat, actual_wat);
    }
}
