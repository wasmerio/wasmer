//! A common functionality used among various compilers.

use core::fmt::Display;
use std::{
    collections::HashMap,
    fs::{self, File},
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

use itertools::Itertools;
use target_lexicon::Architecture;
use tempfile::NamedTempFile;
use wasmer_types::{CompileError, FunctionType, Type};
use which::which;

/// Represents the kind of compiled function or module, used for debugging and identification
/// purposes across multiple compiler backends (e.g., LLVM, Cranelift).
#[derive(Debug, Clone)]
pub enum CompiledKind {
    /// A locally-defined function in the Wasm file.
    Local(String),
    /// A function call trampoline for a given signature.
    FunctionCallTrampoline(FunctionType),
    /// A dynamic function trampoline for a given signature.
    DynamicFunctionTrampoline(FunctionType),
    /// An entire Wasm module.
    Module,
}

/// Converts a slice of `Type` into a string signature, mapping each type to a specific character.
/// Used to represent function signatures in a compact string form.
pub fn types_to_signature(types: &[Type]) -> String {
    let tokens = types
        .iter()
        .map(|ty| match ty {
            Type::I32 => "i",
            Type::I64 => "I",
            Type::F32 => "f",
            Type::F64 => "F",
            Type::V128 => "v",
            Type::ExternRef => "e",
            Type::FuncRef => "r",
            Type::ExceptionRef => "x",
        })
        .collect_vec();
    // Apparently, LLVM has issues if the filename is too long, thus we compact it.
    tokens
        .chunk_by(|a, b| a == b)
        .map(|chunk| {
            if chunk.len() >= 8 {
                format!("{}x{}", chunk.len(), chunk[0])
            } else {
                chunk.to_owned().join("")
            }
        })
        .join("")
}

/// Sanitizes a string so it can be safely used as a filename.
fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Converts a kind into a filename, that we will use to dump
/// the contents of the IR object file to.
pub fn function_kind_to_filename(kind: &CompiledKind) -> String {
    match kind {
        CompiledKind::Local(name) => sanitize_filename(name),
        CompiledKind::FunctionCallTrampoline(func_type) => format!(
            "trampoline_call_{}_{}",
            types_to_signature(func_type.params()),
            types_to_signature(func_type.results())
        ),
        CompiledKind::DynamicFunctionTrampoline(func_type) => format!(
            "trampoline_dynamic_{}_{}",
            types_to_signature(func_type.params()),
            types_to_signature(func_type.results())
        ),
        CompiledKind::Module => "module".into(),
    }
}

#[derive(Debug)]
struct DecodedInsn<'a> {
    offset: usize,
    insn: &'a str,
}

fn parse_instructions(content: &str) -> Result<Vec<DecodedInsn<'_>>, CompileError> {
    content
        .lines()
        .map(|line| line.trim())
        .skip_while(|l| !l.starts_with("0000000000000000"))
        .skip(1)
        .filter(|line| line.trim() != "...")
        .map(|line| -> Result<DecodedInsn<'_>, CompileError> {
            let (offset, insn_part) = line.split_once(':').ok_or(CompileError::Codegen(
                format!("cannot parse objdump line: '{line}'"),
            ))?;
            // instruction content can be empty
            let insn = insn_part
                .trim()
                .split_once('\t')
                .map_or("", |(_data, insn)| insn)
                .trim();
            Ok(DecodedInsn {
                offset: usize::from_str_radix(offset, 16)
                    .map_err(|err| CompileError::Codegen(format!("hex number expected: {err}")))?,
                insn,
            })
        })
        .collect()
}

/// Saves disassembled assembly code to a file with optional comments at specific offsets.
///
/// This function takes raw machine code bytes, disassembles them using `objdump`, and writes
/// the annotated assembly to a file in the specified debug directory.
pub fn save_assembly_to_file<C: Display>(
    arch: Architecture,
    debug_dir: PathBuf,
    function_name: &str,
    body: &[u8],
    assembly_comments: HashMap<usize, C>,
) -> Result<(), CompileError> {
    // Note objdump cannot read from stdin.
    let mut tmpfile = NamedTempFile::new()
        .map_err(|err| CompileError::Codegen(format!("cannot create temporary file: {err}")))?;
    tmpfile
        .write_all(body)
        .map_err(|err| CompileError::Codegen(format!("assembly dump write failed: {err}")))?;
    tmpfile
        .flush()
        .map_err(|err| CompileError::Codegen(format!("flush failed: {err}")))?;

    let objdump_arch = match arch {
        Architecture::X86_64 => "i386:x86-64",
        Architecture::Aarch64(..) => "aarch64",
        _ => {
            return Err(CompileError::Codegen(
                "Assembly dumping is not supported for this architecture".to_string(),
            ));
        }
    };

    if which("objdump").is_err() {
        return Err(CompileError::Codegen(
            "objdump not found in PATH. Please install binutils to use assembly debugging features.".to_string()
        ));
    }

    let command = Command::new("objdump")
        .arg("-b")
        .arg("binary")
        .arg("-m")
        .arg(objdump_arch)
        .arg("-D")
        .arg(tmpfile.path())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|err| CompileError::Codegen(format!("objdump failed: {err}")))?;

    let output = command
        .wait_with_output()
        .map_err(|err| CompileError::Codegen(format!("failed to read stdout: {err}")))?;
    let content = String::from_utf8_lossy(&output.stdout);

    let parsed_instructions = parse_instructions(content.as_ref())?;

    fs::create_dir_all(debug_dir.clone()).map_err(|err| {
        CompileError::Codegen(format!("debug object file creation failed: {err}"))
    })?;

    let mut path = debug_dir;
    path.push(format!("{function_name}.s"));
    let mut file = File::create(path).map_err(|err| {
        CompileError::Codegen(format!("debug object file creation failed: {err}"))
    })?;

    file.write_all(format!(";; {function_name}\n\n").as_bytes())
        .map_err(|err| {
            CompileError::Codegen(format!("cannot write content to object file: {err}"))
        })?;

    // Dump the instruction annotated with the comments.
    for insn in parsed_instructions {
        if let Some(comment) = assembly_comments.get(&insn.offset) {
            file.write_all(format!("      \t\t;; {comment}\n").as_bytes())
                .map_err(|err| {
                    CompileError::Codegen(format!("cannot write content to object file: {err}"))
                })?;
        }
        file.write_all(format!("{:6x}:\t\t{}\n", insn.offset, insn.insn).as_bytes())
            .map_err(|err| {
                CompileError::Codegen(format!("cannot write content to object file: {err}"))
            })?;
    }

    Ok(())
}
