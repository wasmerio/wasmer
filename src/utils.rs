//! Utility functions for the WebAssembly module

use wasmer_runtime::{types::Type, Module, Value};
use wasmer_runtime_core::{backend::SigRegistry, module::ExportIndex};

/// Detect if a provided binary is a Wasm file
pub fn is_wasm_binary(binary: &[u8]) -> bool {
    binary.starts_with(&[b'\0', b'a', b's', b'm'])
}

#[derive(Debug, Clone)]
pub enum InvokeError {
    CouldNotFindFunction,
    ExportNotFunction,
    WrongNumArgs { expected: u16, found: u16 },
    CouldNotParseArg(String),
}

/// Parses arguments for the `--invoke` flag on the run command
pub fn parse_args(
    module: &Module,
    fn_name: &str,
    args: &[String],
) -> Result<Vec<Value>, InvokeError> {
    let export_index = module
        .info()
        .exports
        .get(fn_name)
        .ok_or(InvokeError::CouldNotFindFunction)?;

    let signature = if let ExportIndex::Func(func_index) = export_index {
        let sig_index = module
            .info()
            .func_assoc
            .get(*func_index)
            .expect("broken invariant, incorrect func index");
        SigRegistry.lookup_signature_ref(&module.info().signatures[*sig_index])
    } else {
        return Err(InvokeError::ExportNotFunction);
    };

    let parameter_types = signature.params();
    let mut arg_error = None;

    if args.len() != parameter_types.len() {
        return Err(InvokeError::WrongNumArgs {
            expected: parameter_types.len() as _,
            found: args.len() as _,
        });
    } else {
        args.iter()
            .enumerate()
            .try_fold(
                Vec::with_capacity(args.len()),
                |mut accumulator, (nth, argument)| {
                    if let Some(value) = match parameter_types[nth] {
                        Type::I32 => argument
                            .parse::<i32>()
                            .map(|v| Some(Value::I32(v)))
                            .unwrap_or_else(|_| {
                                arg_error = Some(InvokeError::CouldNotParseArg(format!(
                                    "Failed to parse `{:?}` as an `i32`",
                                    argument
                                )));
                                None
                            }),
                        Type::I64 => argument
                            .parse::<i64>()
                            .map(|v| Some(Value::I64(v)))
                            .unwrap_or_else(|_| {
                                arg_error = Some(InvokeError::CouldNotParseArg(format!(
                                    "Failed to parse `{:?}` as an `i64`",
                                    argument
                                )));
                                None
                            }),
                        Type::V128 => argument
                            .parse::<u128>()
                            .map(|v| Some(Value::V128(v)))
                            .unwrap_or_else(|_| {
                                arg_error = Some(InvokeError::CouldNotParseArg(format!(
                                    "Failed to parse `{:?}` as an `i128`",
                                    argument
                                )));
                                None
                            }),
                        Type::F32 => argument
                            .parse::<f32>()
                            .map(|v| Some(Value::F32(v)))
                            .unwrap_or_else(|_| {
                                arg_error = Some(InvokeError::CouldNotParseArg(format!(
                                    "Failed to parse `{:?}` as an `f32`",
                                    argument
                                )));
                                None
                            }),
                        Type::F64 => argument
                            .parse::<f64>()
                            .map(|v| Some(Value::F64(v)))
                            .unwrap_or_else(|_| {
                                arg_error = Some(InvokeError::CouldNotParseArg(format!(
                                    "Failed to parse `{:?}` as an `f64`",
                                    argument
                                )));
                                None
                            }),
                    } {
                        accumulator.push(value);

                        Some(accumulator)
                    } else {
                        None
                    }
                },
            )
            .map_or_else(
                || Err(arg_error.unwrap()),
                |arguments: Vec<Value>| Ok(arguments),
            )
    }
}
