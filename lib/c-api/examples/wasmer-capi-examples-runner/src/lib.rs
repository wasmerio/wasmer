#[cfg(test)]
use std::error::Error;

#[cfg(test)]
static INCLUDE_REGEX: &str = "#include \"(.*)\"";

#[derive(Default)]
pub struct RemoveTestsOnDrop {}

impl Drop for RemoveTestsOnDrop {
    fn drop(&mut self) {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        for entry in std::fs::read_dir(manifest_dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let extension = path.extension().and_then(|s| s.to_str());
            if extension == Some("obj") || extension == Some("exe") || extension == Some("o") {
                println!("removing {}", path.display());
                let _ = std::fs::remove_file(&path);
            }
        }
        if let Some(parent) = std::path::Path::new(&manifest_dir).parent() {
            for entry in std::fs::read_dir(parent).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                let extension = path.extension().and_then(|s| s.to_str());
                if extension == Some("obj") || extension == Some("exe") || extension == Some("o") {
                    println!("removing {}", path.display());
                    let _ = std::fs::remove_file(path);
                }
            }
        }
    }
}

fn make_package() {
    let wasmer_root_dir = find_wasmer_base_dir();
    let _ = std::fs::create_dir_all(format!("{wasmer_root_dir}/package/lib"));
    let _ = std::fs::create_dir_all(format!("{wasmer_root_dir}/package/include"));
    let _ = std::fs::copy(
        format!("{wasmer_root_dir}/lib/c-api/tests/wasm.h"),
        format!("{wasmer_root_dir}/package/include/wasm.h"),
    );
    let _ = std::fs::copy(
        format!("{wasmer_root_dir}/lib/c-api/tests/wasmer.h"),
        format!("{wasmer_root_dir}/package/include/wasmer.h"),
    );
    #[cfg(target_os = "windows")]
    let _ = std::fs::copy(
        &format!("{wasmer_root_dir}/target/release/wasmer.dll"),
        &format!("{wasmer_root_dir}/package/lib"),
    );
    #[cfg(target_os = "windows")]
    let _ = std::fs::copy(
        &format!("{wasmer_root_dir}/target/release/wasmer.dll.lib"),
        &format!("{wasmer_root_dir}/package/lib"),
    );
    #[cfg(not(target_os = "windows"))]
    let _ = std::fs::copy(
        format!("{wasmer_root_dir}/target/release/libwasmer.so"),
        format!("{wasmer_root_dir}/package/lib"),
    );
    #[cfg(not(target_os = "windows"))]
    let _ = std::fs::copy(
        format!("{wasmer_root_dir}/target/release/libwasmer.lib"),
        format!("{wasmer_root_dir}/package/lib"),
    );
    println!("copying done (make package)");
}

#[derive(Debug)]
pub struct Config {
    pub wasmer_dir: String,
    pub root_dir: String,
}

impl Config {
    pub fn get() -> Config {
        let mut config = Config {
            wasmer_dir: std::env::var("WASMER_DIR").unwrap_or_default(),
            root_dir: std::env::var("ROOT_DIR").unwrap_or_default(),
        };

        // resolve the path until the /wasmer root directory
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let wasmer_base_dir = find_wasmer_base_dir();

        if config.wasmer_dir.is_empty() {
            println!("manifest dir = {manifest_dir}, wasmer root dir = {wasmer_base_dir}");
            config.wasmer_dir = wasmer_base_dir.clone() + "/package";
            if !std::path::Path::new(&config.wasmer_dir).exists() {
                println!("running make build-capi...");
                // run make build-capi
                let mut cmd = std::process::Command::new("make");
                cmd.arg("build-capi");
                cmd.current_dir(&wasmer_base_dir);
                let result = cmd.output();
                println!("make build-capi: {result:#?}");

                println!("running make package-capi...");
                // run make package
                let mut cmd = std::process::Command::new("make");
                cmd.arg("package-capi");
                cmd.current_dir(&wasmer_base_dir);
                let result = cmd.output();
                make_package();
                println!("make package: {result:#?}");

                println!("list {}", config.wasmer_dir);
                match std::fs::read_dir(&config.wasmer_dir) {
                    Ok(o) => {
                        for entry in o {
                            let entry = entry.unwrap();
                            let path = entry.path();
                            println!("    {:?}", path.file_name());
                        }
                    }
                    Err(e) => {
                        println!("error in reading config.wasmer_dir: {e}");
                    }
                };
            }
        }
        if config.root_dir.is_empty() {
            config.root_dir = wasmer_base_dir + "/lib/c-api/examples";
        }

        config
    }
}

fn find_wasmer_base_dir() -> String {
    let wasmer_base_dir = env!("CARGO_MANIFEST_DIR");
    let mut path2 = wasmer_base_dir.split("wasmer").collect::<Vec<_>>();
    path2.pop();
    let mut wasmer_base_dir = path2.join("wasmer");

    if wasmer_base_dir.contains("wasmer/lib/c-api") {
        wasmer_base_dir = wasmer_base_dir
            .split("wasmer/lib/c-api")
            .next()
            .unwrap()
            .to_string()
            + "wasmer";
    } else if wasmer_base_dir.contains("wasmer\\lib\\c-api") {
        wasmer_base_dir = wasmer_base_dir
            .split("wasmer\\lib\\c-api")
            .next()
            .unwrap()
            .to_string()
            + "wasmer";
    }

    wasmer_base_dir
}

#[cfg(test)]
pub const TESTS: &[&str] = &[
    "deprecated-header",
    "early-exit",
    "instance",
    "imports-exports",
    "exports-function",
    "exports-global",
    "memory",
    "memory2",
    "features",
    "wasi",
];

#[test]
fn test_run() {
    let _drop = RemoveTestsOnDrop::default();
    let config = Config::get();
    println!("config: {:#?}", config);

    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let host = target_lexicon::HOST.to_string();
    let target = &host;

    let wasmer_dll_dir = format!("{}/lib", config.wasmer_dir);
    let libwasmer_so_path = format!("{}/lib/libwasmer.so", config.wasmer_dir);
    let path = std::env::var("PATH").unwrap_or_default();
    let newpath = format!("{wasmer_dll_dir};{path}");
    let exe_dir = match std::path::Path::new(&manifest_dir).parent() {
        Some(parent) => format!("{}", parent.display()),
        None => manifest_dir.to_string(),
    };

    for test in TESTS.iter() {
        let manifest_dir_parent = std::path::Path::new(&manifest_dir);
        let manifest_dir_parent = manifest_dir_parent.parent().unwrap();
        let c_file_path = manifest_dir_parent.join(&format!("{test}.c"));

        if target.contains("msvc") {
            let mut build = cc::Build::new();
            let build = build
                .cargo_metadata(false)
                .warnings(true)
                .static_crt(true)
                .extra_warnings(true)
                .warnings_into_errors(false)
                .debug(true)
                .host(&host)
                .target(target)
                .opt_level(1);

            let compiler = build.try_get_compiler().unwrap();
            let mut command = compiler.to_command();

            command.arg(&format!("{}", c_file_path.display()));
            if !config.wasmer_dir.is_empty() {
                command.arg("/I");
                command.arg(&format!("{}/include/", config.wasmer_dir));
                let mut log = String::new();
                fixup_symlinks(
                    &[
                        format!("{}/include", config.wasmer_dir),
                        config.root_dir.to_string(),
                    ],
                    &mut log,
                    &config.root_dir,
                )
                .unwrap_or_else(|_| panic!("failed to fix symlinks: {log}"));
                println!("{log}");
            }

            let exe_outpath = manifest_dir_parent.join(&format!("{test}.exe"));
            let exe_outpath = format!("{}", exe_outpath.display());
            println!("compiling exe to {exe_outpath}");

            command.arg(&format!("/Fo:{}/", manifest_dir_parent.display()));
            command.arg("/link");
            if !config.wasmer_dir.is_empty() {
                command.arg(&format!("/LIBPATH:{}/lib", config.wasmer_dir));
                command.arg(&format!("{}/lib/wasmer.dll.lib", config.wasmer_dir));
            }
            command.arg(&format!("/OUT:{exe_outpath}"));

            // read vcvars into file, append command, then execute the bat

            println!("compiling WINDOWS {test}: {command:?}");

            let vcvars_bat_path = find_vcvarsall(&compiler).expect("no vcvarsall.bat");
            let vcvars_bat_path_parent = std::path::Path::new(&vcvars_bat_path).parent().unwrap();
            let _vcvars_modified_output = vcvars_bat_path_parent.join("compile-windows.bat");
            let vcvars_bat_file = std::fs::read_to_string(&vcvars_bat_path).unwrap();
            let batch_formatted = format!("{}\\", vcvars_bat_path_parent.display());
            let vcvars_bat_file = vcvars_bat_file
                .replace("%~dp0", &batch_formatted.replace('\\', "\\\\"))
                .replace("\"%1\"", "\"x64\"");
            let vcvars_modified = format!("{vcvars_bat_file}\r\n{command:?}");
            let path = std::path::Path::new(&manifest_dir).join("compile-windows.bat");
            println!("outputting batch to {}", path.display());
            std::fs::write(&path, vcvars_modified).unwrap();

            // print_wasmer_root_to_stdout(&config);

            let mut vcvars = std::process::Command::new("cmd");
            vcvars.arg("/C");
            vcvars.arg(&path);
            vcvars.arg("x64");
            vcvars.current_dir(vcvars_bat_path_parent);

            // compile
            let output = vcvars
                .output()
                .map_err(|e| format!("failed to compile {command:#?}: {e}"))
                .unwrap();
            if !output.status.success() {
                println!("stdout: {}", String::from_utf8_lossy(&output.stdout));
                println!("stdout: {}", String::from_utf8_lossy(&output.stderr));
                // print_wasmer_root_to_stdout(&config);
                panic!("failed to compile {test}");
            }

            if !std::path::Path::new(&exe_outpath).exists() {
                panic!("error: {exe_outpath} does not exist");
            }
            if !std::path::Path::new(&wasmer_dll_dir)
                .join("wasmer.dll")
                .exists()
            {
                panic!("error: {wasmer_dll_dir} has no wasmer.dll");
            }
            // execute
            let mut command = std::process::Command::new(&exe_outpath);
            command.env("PATH", &newpath);
            command.current_dir(&exe_dir);
            println!("executing {test}: {command:?}");
            println!("setting current dir = {exe_dir}");
            let output = command
                .output()
                .unwrap_or_else(|_| panic!("failed to run {command:#?}"));
            if !output.status.success() {
                println!("{output:#?}");
                println!("stdout: {}", String::from_utf8_lossy(&output.stdout));
                println!("stderr: {}", String::from_utf8_lossy(&output.stderr));
                // print_wasmer_root_to_stdout(&config);
                panic!("failed to execute {test}");
            }
        } else {
            let compiler_cmd = match std::process::Command::new("cc").output() {
                Ok(_) => "cc",
                Err(_) => "gcc",
            };

            let mut command = std::process::Command::new(compiler_cmd);

            if !config.wasmer_dir.is_empty() {
                command.arg("-I");
                command.arg(&format!("{}/include", config.wasmer_dir));
                let mut log = String::new();
                fixup_symlinks(
                    &[
                        format!("{}/include", config.wasmer_dir),
                        config.root_dir.to_string(),
                    ],
                    &mut log,
                    &config.root_dir,
                )
                .unwrap_or_else(|_| panic!("failed to fix symlinks: {log}"));
            }
            command.arg(&c_file_path);
            if !config.wasmer_dir.is_empty() {
                command.arg("-L");
                command.arg(&format!("{}/lib/", config.wasmer_dir));
                command.arg("-lwasmer");
                command.arg(&format!("-Wl,-rpath,{}/lib/", config.wasmer_dir));
            }
            command.arg("-o");
            command.arg(&format!("{manifest_dir}/../{test}"));

            // cc -g -IC:/Users/felix/Development/wasmer/lib/c-api/examples/../tests
            //       -IC:/Users/felix/Development/wasmer/package/include
            //       -c -o deprecated-header.o deprecated-header.c

            /*
                cc -I /home/runner/work/wasmer/wasmer/lib/c-api/tests
                -I" "/home/runner/work/wasmer/wasmer/package/include"
                "/home/runner/work/wasmer/wasmer/lib/c-api/tests/wasmer-c-api-test-runner/../wasm-c-api/example/callback.c"
                "-L/home/runner/work/wasmer/wasmer/package/lib"
                "-lwasmer"
                "-o" "/home/runner/work/wasmer/wasmer/lib/c-api/tests/wasmer-c-api-test-runner/../wasm-c-api/example/callback"
            */

            println!("compiling LINUX {command:#?}");
            // compile
            let output = command
                .output()
                .map_err(|e| format!("failed to compile {command:#?}: {e}"))
                .unwrap();
            if !output.status.success() {
                println!("stdout: {}", String::from_utf8_lossy(&output.stdout));
                println!("stderr: {}", String::from_utf8_lossy(&output.stderr));
                // print_wasmer_root_to_stdout(&config);
                panic!("failed to compile {test}: {command:#?}");
            }

            // execute
            let mut command = std::process::Command::new(&format!("{manifest_dir}/../{test}"));
            command.env("LD_PRELOAD", &libwasmer_so_path);
            command.current_dir(&exe_dir);
            println!("execute: {command:#?}");
            let output = command
                .output()
                .unwrap_or_else(|_| panic!("failed to run {command:#?}"));
            if !output.status.success() {
                println!("stdout: {}", String::from_utf8_lossy(&output.stdout));
                println!("stdout: {}", String::from_utf8_lossy(&output.stderr));
                // print_wasmer_root_to_stdout(&config);
                panic!("failed to execute {test} executable");
            }
        }
    }
}

#[cfg(test)]
fn print_wasmer_root_to_stdout(config: &Config) {
    println!("print_wasmer_root_to_stdout");

    use walkdir::WalkDir;

    for entry in WalkDir::new(&config.wasmer_dir)
        .into_iter()
        .filter_map(Result::ok)
    {
        let f_name = String::from(entry.path().canonicalize().unwrap().to_string_lossy());
        println!("{f_name}");
    }

    for entry in WalkDir::new(&config.root_dir)
        .into_iter()
        .filter_map(Result::ok)
    {
        let f_name = String::from(entry.path().canonicalize().unwrap().to_string_lossy());
        println!("{f_name}");
    }

    println!("printed");
}

#[cfg(test)]
fn fixup_symlinks(
    include_paths: &[String],
    log: &mut String,
    root_dir: &str,
) -> Result<(), Box<dyn Error>> {
    let source = std::path::Path::new(root_dir)
        .join("lib")
        .join("c-api")
        .join("tests")
        .join("wasm-c-api")
        .join("include")
        .join("wasm.h");
    let target = std::path::Path::new(root_dir)
        .join("lib")
        .join("c-api")
        .join("tests")
        .join("wasm.h");
    println!("copying {} -> {}", source.display(), target.display());
    let _ = std::fs::copy(source, target);

    log.push_str(&format!("include paths: {include_paths:?}"));
    for i in include_paths {
        let i = i.replacen("-I", "", 1);
        let mut paths_headers = Vec::new();
        let readdir = match std::fs::read_dir(&i) {
            Ok(o) => o,
            Err(_) => continue,
        };
        for entry in readdir {
            let entry = entry?;
            let path = entry.path();
            let path_display = format!("{}", path.display());
            if path_display.ends_with('h') {
                paths_headers.push(path_display);
            }
        }
        fixup_symlinks_inner(&paths_headers, log)?;
    }

    Ok(())
}

#[cfg(test)]
fn fixup_symlinks_inner(include_paths: &[String], log: &mut String) -> Result<(), Box<dyn Error>> {
    log.push_str(&format!("fixup symlinks: {include_paths:#?}"));
    let regex = regex::Regex::new(INCLUDE_REGEX).unwrap();
    for path in include_paths.iter() {
        let file = match std::fs::read_to_string(path) {
            Ok(o) => o,
            _ => continue,
        };
        let lines_3 = file.lines().take(3).collect::<Vec<_>>();
        log.push_str(&format!("first 3 lines of {path:?}: {:#?}\n", lines_3));

        let parent = std::path::Path::new(&path).parent().unwrap();
        if let Ok(symlink) = std::fs::read_to_string(parent.join(&file)) {
            log.push_str(&format!("symlinking {path:?}\n"));
            std::fs::write(path, symlink)?;
        }

        // follow #include directives and recurse
        let filepaths = regex
            .captures_iter(&file)
            .map(|c| c[1].to_string())
            .collect::<Vec<_>>();
        log.push_str(&format!("regex captures: ({path:?}): {:#?}\n", filepaths));
        let joined_filepaths = filepaths
            .iter()
            .map(|s| {
                let path = parent.join(s);
                format!("{}", path.display())
            })
            .collect::<Vec<_>>();
        fixup_symlinks_inner(&joined_filepaths, log)?;
    }
    Ok(())
}

#[cfg(test)]
fn find_vcvarsall(compiler: &cc::Tool) -> Option<String> {
    if !compiler.is_like_msvc() {
        return None;
    }

    let path = compiler.path();
    let path = format!("{}", path.display());
    let split = path.split("VC").next()?;

    Some(format!("{split}VC\\Auxiliary\\Build\\vcvarsall.bat"))
}
