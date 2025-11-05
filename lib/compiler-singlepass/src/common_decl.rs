use std::{
    collections::HashMap,
    fs::{self, File},
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

use tempfile::NamedTempFile;
use wasmer_types::CompileError;
use which::which;

use crate::{codegen_error, machine::AssemblyComment};

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Size {
    S8,
    S16,
    S32,
    S64,
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

pub(crate) fn save_assembly_to_file(
    debug_dir: PathBuf,
    function_name: &str,
    body: &[u8],
    assembly_comments: HashMap<usize, AssemblyComment>,
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

    #[cfg(target_arch = "x86_64")]
    let objdump_arch = "i386:x86-64";
    #[cfg(target_arch = "aarch64")]
    let objdump_arch = "aarch64";
    #[cfg(target_arch = "riscv64")]
    let objdump_arch = "riscv:rv64";
    #[cfg(not(any(
        target_arch = "x86_64",
        target_arch = "aarch64",
        target_arch = "riscv64"
    )))]
    {
        return Err(CompileError::Codegen(
            "Assembly dumping is not supported for this architecture".to_string(),
        ));
    }

    if which("objdump").is_err() {
        codegen_error!(
            "objdump not found in PATH. Please install binutils to use assembly debugging features."
        );
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
