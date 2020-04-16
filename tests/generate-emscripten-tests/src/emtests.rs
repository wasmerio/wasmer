//! This file will run at build time to autogenerate the Emscripten tests
//! It will compile the files indicated in TESTS, to:executable and .wasm
//! - Compile using cc and get the output from it (expected output)
//! - Compile using emcc and get the .wasm from it (wasm)
use glob::glob;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use std::fs::File;

const EXTENSIONS: [&str; 2] = ["c", "cpp"];
const EXCLUDES: [&str; 0] = [];

pub fn compile(file: &str) {
    let mut output_path = PathBuf::from(file);
    output_path.set_extension("out");
    let output_str = output_path.to_str().unwrap();

    println!("Compiling Emscripten file natively: {}", file);

    // Compile to .out
    Command::new("cc")
        .arg(file)
        .arg("-o")
        .arg(output_str)
        .output()
        .expect("failed to execute cc command");

    // Get the result of .out
    let exec_output = Command::new(output_str).arg(output_str).output();

    // Is fine if _output fails, as the original file might have
    // `EM_ASM` javascript definitions inside. In that case,
    // the program can't be run natively without JS, and we just
    // want to skip any further execution.
    if exec_output.is_err() {
        println!("  -> Can't execute the file natively. Skipping emcc");
        return;
    } else {
        exec_output.unwrap();
    }

    // Remove executable, we don't care if it's successful or not
    drop(fs::remove_file(output_str));

    let mut output_path = PathBuf::from(file);
    output_path.set_extension("js");
    let output_str = output_path.to_str().unwrap();

    let wasm_file_metadata = {
        let mut wasm_file_path = PathBuf::from(file);
        wasm_file_path.set_extension("wasm");
        File::open(wasm_file_path).and_then(|wf| wf.metadata()).ok()
    };

    let real_file = File::open(file).unwrap();
    let file_metadata = real_file.metadata().unwrap();
    if wasm_file_metadata.is_none()
        || file_metadata.modified().unwrap() >= wasm_file_metadata.unwrap().modified().unwrap()
    {
        // Compile to wasm
        println!("Compiling Emscripten file: {}", file);

        let _wasm_compilation = Command::new("emcc")
            .arg(file)
            .arg("-s")
            .arg("WASM=1")
            .arg("-o")
            .arg(output_str)
            .output()
            .expect("failed to execute emcc process. Is `emcc` available in your system?");

        // panic!("{:?}", wasm_compilation);
        // if output.stderr {
        //     panic!("{}", output.stderr);
        // }

        // Remove js file
        if Path::new(output_str).is_file() {
            fs::remove_file(output_str).unwrap();
        } else {
            println!("Output JS not found: {}", output_str);
        }
    }
}

pub fn build() {
    for ext in EXTENSIONS.iter() {
        for entry in glob(&format!(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../emscripten_resources/emtests/*.{}"
            ),
            ext
        ))
        .unwrap()
        {
            match entry {
                Ok(path) => {
                    let test = path.to_str().unwrap();
                    if !EXCLUDES.iter().any(|e| test.ends_with(e)) {
                        compile(test);
                    }
                }
                Err(e) => println!("{:?}", e),
            }
        }
    }
}
