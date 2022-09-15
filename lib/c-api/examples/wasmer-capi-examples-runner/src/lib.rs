#[cfg(test)]
use std::error::Error;

#[cfg(test)]
static INCLUDE_REGEX: &str = "#include \"(.*)\"";

#[derive(Default)]
pub struct RemoveTestsOnDrop {}

impl Drop for RemoveTestsOnDrop {
    fn drop(&mut self) {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        for entry in std::fs::read_dir(&manifest_dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let extension = path.extension().and_then(|s| s.to_str());
            if extension == Some("obj") || extension == Some("exe") || extension == Some("o") {
                println!("removing {}", path.display());
                let _ = std::fs::remove_file(&path);
            }
        }
        if let Some(parent) = std::path::Path::new(&manifest_dir).parent() {
            for entry in std::fs::read_dir(&parent).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                let extension = path.extension().and_then(|s| s.to_str());
                if extension == Some("obj") || extension == Some("exe") || extension == Some("o") {
                    println!("removing {}", path.display());
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct Config {
    pub wasmer_dir: String,
    pub root_dir: String,
    // linux + mac
    pub cflags: String,
    pub ldflags: String,
    pub ldlibs: String,
    // windows msvc
    pub msvc_cflags: String,
    pub msvc_ldflags: String,
    pub msvc_ldlibs: String,
}

impl Config {
    pub fn get() -> Config {
        let mut config = Config {
            wasmer_dir: std::env::var("WASMER_DIR").unwrap_or_default(),
            root_dir: std::env::var("ROOT_DIR").unwrap_or_default(),

            cflags: std::env::var("CFLAGS").unwrap_or_default(),
            ldflags: std::env::var("LDFLAGS").unwrap_or_default(),
            ldlibs: std::env::var("LDLIBS").unwrap_or_default(),

            msvc_cflags: std::env::var("MSVC_CFLAGS").unwrap_or_default(),
            msvc_ldflags: std::env::var("MSVC_LDFLAGS").unwrap_or_default(),
            msvc_ldlibs: std::env::var("MSVC_LDLIBS").unwrap_or_default(),
        };

        // resolve the path until the /wasmer root directory
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let wasmer_base_dir = env!("CARGO_MANIFEST_DIR");
        let mut path2 = wasmer_base_dir.split("wasmer").collect::<Vec<_>>();
        path2.pop();
        let wasmer_base_dir = path2.join("wasmer");

        if config.wasmer_dir.is_empty() {
            println!("manifest dir = {manifest_dir}, wasmer root dir = {wasmer_base_dir}");
            config.wasmer_dir = wasmer_base_dir.clone() + "wasmer/package";
        }
        if config.root_dir.is_empty() {
            config.root_dir = wasmer_base_dir + "wasmer/lib/c-api/examples";
        }

        config
    }
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

    // cc -g -IC:/Users/felix/Development/wasmer/lib/c-api/examples/../tests
    //       -IC:/Users/felix/Development/wasmer/package/include
    //       -c -o deprecated-header.o deprecated-header.c

    if target.contains("msvc") {
        for test in TESTS.iter() {
            let mut build = cc::Build::new();
            let mut build = build
                .cargo_metadata(false)
                .warnings(true)
                .static_crt(true)
                .extra_warnings(true)
                .warnings_into_errors(false)
                .debug(config.ldflags.contains("-g"))
                .host(&host)
                .target(target)
                .opt_level(1);

            let compiler = build.try_get_compiler().unwrap();
            let mut command = compiler.to_command();

            command.arg(&format!("{manifest_dir}/../{test}.c"));
            if !config.msvc_cflags.is_empty() {
                command.arg(config.msvc_cflags.clone());
            } else if !config.wasmer_dir.is_empty() {
                command.arg("/I");
                command.arg(&format!("{}/include", config.wasmer_dir));
                let mut log = String::new();
                fixup_symlinks(&[format!("{}/include", config.wasmer_dir)], &mut log)
                    .expect(&format!("failed to fix symlinks: {log}"));
                println!("{log}");
            }
            command.arg("/link");
            if !config.msvc_ldlibs.is_empty() {
                command.arg(config.msvc_ldlibs.clone());
            } else if !config.wasmer_dir.is_empty() {
                command.arg(&format!("/LIBPATH:{}/lib", config.wasmer_dir));
                command.arg(&format!("{}/lib/wasmer.dll.lib", config.wasmer_dir));
            }
            let wasmer_dll_dir = format!("{}/lib", config.wasmer_dir);
            command.arg(&format!("/OUT:\"{manifest_dir}/../{test}.exe\""));

            let exe_dir = match std::path::Path::new(&manifest_dir).parent() {
                Some(parent) => format!("{}", parent.display()),
                None => format!("{manifest_dir}"),
            };

            // run vcvars
            let vcvars_bat_path = find_vcvars64(&compiler).expect("no vcvars64.bat");
            let mut vcvars = std::process::Command::new("cmd");
            vcvars.arg("/C");
            vcvars.arg(vcvars_bat_path);
            println!("running {vcvars:?}");

            // cmd /C vcvars64.bat
            let output = vcvars
                .output()
                .expect("could not invoke vcvars64.bat at {vcvars_bat_path}");

            if !output.status.success() {
                println!();
                println!("stdout: {}", String::from_utf8_lossy(&output.stdout));
                println!("stderr: {}", String::from_utf8_lossy(&output.stderr));
                panic!("failed to invoke vcvars64.bat {test}");
            }

            println!("compiling {test}: {command:?}");

            // compile
            let output = command
                .output()
                .expect(&format!("failed to compile {command:#?}"));
            if !output.status.success() {
                println!("stdout: {}", String::from_utf8_lossy(&output.stdout));
                println!("stdout: {}", String::from_utf8_lossy(&output.stderr));
                panic!("failed to compile {test}");
            }

            let path = std::env::var("PATH").unwrap_or_default();
            let newpath = format!("{wasmer_dll_dir};{path}");

            // execute
            let mut command = std::process::Command::new(&format!("{manifest_dir}/../{test}.exe"));
            command.env("PATH", newpath.clone());
            command.current_dir(exe_dir.clone());
            println!("executing {test}: {command:?}");
            println!("setting current dir = {exe_dir}");
            let output = command
                .output()
                .expect(&format!("failed to run {command:#?}"));
            if !output.status.success() {
                println!("stdout: {}", String::from_utf8_lossy(&output.stdout));
                println!("stdout: {}", String::from_utf8_lossy(&output.stderr));
                panic!("failed to execute {test}");
            }
        }
    } else {
        for test in TESTS.iter() {
            let mut command = std::process::Command::new("cc");

            if !config.cflags.is_empty() {
                command.arg(config.cflags.clone());
            } else if !config.wasmer_dir.is_empty() {
                command.arg("-I");
                command.arg(&format!("{}/include", config.wasmer_dir));
            }
            command.arg(config.ldflags.clone());
            command.arg(&format!("{manifest_dir}/../{test}.c"));
            if !config.ldlibs.is_empty() {
                command.arg(config.ldlibs.clone());
            } else if !config.wasmer_dir.is_empty() {
                command.arg(&format!("-L{}/lib", config.wasmer_dir));
                command.arg(&format!("-lwasmer"));
            }
            command.arg("-o");
            command.arg(&format!("{manifest_dir}/../{test}"));

            // compile
            let output = command
                .output()
                .expect(&format!("failed to compile {command:#?}"));
            if !output.status.success() {
                println!("stdout: {}", String::from_utf8_lossy(&output.stdout));
                println!("stdout: {}", String::from_utf8_lossy(&output.stderr));
                panic!("failed to compile {test}");
            }

            // execute
            let mut command = std::process::Command::new(&format!("{manifest_dir}/../{test}"));
            let output = command
                .output()
                .expect(&format!("failed to run {command:#?}"));
            if !output.status.success() {
                println!("stdout: {}", String::from_utf8_lossy(&output.stdout));
                println!("stdout: {}", String::from_utf8_lossy(&output.stderr));
                panic!("failed to execute {test}");
            }
        }
    }
}

#[cfg(test)]
fn fixup_symlinks(include_paths: &[String], log: &mut String) -> Result<(), Box<dyn Error>> {
    log.push_str(&format!("include paths: {include_paths:?}"));
    for i in include_paths {
        let i = i.replacen("-I", "", 1);
        let mut paths_headers = Vec::new();
        for entry in std::fs::read_dir(&i)? {
            let entry = entry?;
            let path = entry.path();
            let path_display = format!("{}", path.display());
            if path_display.ends_with("h") {
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
        let file = std::fs::read_to_string(&path)?;
        let lines_3 = file.lines().take(3).collect::<Vec<_>>();
        log.push_str(&format!("first 3 lines of {path:?}: {:#?}\n", lines_3));

        let parent = std::path::Path::new(&path).parent().unwrap();
        if let Ok(symlink) = std::fs::read_to_string(parent.clone().join(&file)) {
            log.push_str(&format!("symlinking {path:?}\n"));
            std::fs::write(&path, symlink)?;
        }

        // follow #include directives and recurse
        let filepaths = regex
            .captures_iter(&file)
            .map(|c| c[1].to_string())
            .collect::<Vec<_>>();
        log.push_str(&format!("regex captures: ({path:?}): {:#?}\n", filepaths));
        let joined_filepaths = filepaths
            .iter()
            .filter_map(|s| {
                let path = parent.clone().join(s);
                Some(format!("{}", path.display()))
            })
            .collect::<Vec<_>>();
        fixup_symlinks_inner(&joined_filepaths, log)?;
    }
    Ok(())
}

#[cfg(test)]
fn find_vcvars64(compiler: &cc::Tool) -> Option<String> {
    if !compiler.is_like_msvc() {
        return None;
    }

    let path = compiler.path();
    let path = format!("{}", path.display());
    let split = path.split("VC").nth(0)?;

    Some(format!("{split}VC\\Auxiliary\\Build\\vcvars64.bat"))
}
