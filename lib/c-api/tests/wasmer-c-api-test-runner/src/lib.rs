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
        Config {
            wasmer_dir: std::env::var("WASMER_DIR").unwrap_or_default(),
            root_dir: std::env::var("ROOT_DIR").unwrap_or_default(),

            cflags: std::env::var("CFLAGS").unwrap_or_default(),
            ldflags: std::env::var("LDFLAGS").unwrap_or_default(),
            ldlibs: std::env::var("LDLIBS").unwrap_or_default(),

            msvc_cflags: std::env::var("MSVC_CFLAGS").unwrap_or_default(),
            msvc_ldflags: std::env::var("MSVC_LDFLAGS").unwrap_or_default(),
            msvc_ldlibs: std::env::var("MSVC_LDLIBS").unwrap_or_default(),
        }
    }
}

#[derive(Default)]
pub struct RemoveTestsOnDrop {}

impl Drop for RemoveTestsOnDrop {
    fn drop(&mut self) {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        for entry in std::fs::read_dir(&manifest_dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let extension = path.extension().and_then(|s| s.to_str());
            if extension == Some("obj") || extension == Some("exe") {
                println!("removing {}", path.display());
                let _ = std::fs::remove_file(&path);
            }
        }
        if let Some(parent) = std::path::Path::new(&manifest_dir).parent() {
            for entry in std::fs::read_dir(&parent).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                let extension = path.extension().and_then(|s| s.to_str());
                if extension == Some("obj") || extension == Some("exe") {
                    println!("removing {}", path.display());
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
    }
}

const CAPI_BASE_TESTS: &[&str] = &[
    "wasm-c-api/example/callback",
    "wasm-c-api/example/memory",
    "wasm-c-api/example/start",
    "wasm-c-api/example/global",
    "wasm-c-api/example/reflect",
    "wasm-c-api/example/trap",
    "wasm-c-api/example/hello",
    "wasm-c-api/example/serialize",
    "wasm-c-api/example/multi",
];

const CAPI_BASE_TESTS_NOT_WORKING: &[&str] = &[
    "wasm-c-api/example/finalize",
    "wasm-c-api/example/hostref",
    "wasm-c-api/example/threads",
    "wasm-c-api/example/table",
];

// Runs all the tests that are working in the /c directory
#[test]
fn test_ok() {
    let _drop = RemoveTestsOnDrop::default();
    let config = Config::get();
    println!("config: {:#?}", config);

    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let host = target_lexicon::HOST.to_string();
    let target = &host;

    if target.contains("msvc") {
        for test in CAPI_BASE_TESTS.iter() {
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

            let exe_dir = format!("{manifest_dir}/../wasm-c-api/example");

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
                println!("");
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

            // cc -g -IC:/Users/felix/Development/wasmer/lib/c-api/tests/
            //          -IC:/Users/felix/Development/wasmer/package/include
            //
            //          -Wl,-rpath,C:/Users/felix/Development/wasmer/package/lib
            //
            //          wasm-c-api/example/callback.c
            //
            //          -LC:/Users/felix/Development/wasmer/package/lib -lwasmer
            //
            // -o wasm-c-api/example/callback
        }
    } else {
        for test in CAPI_BASE_TESTS.iter() {
            let mut command = std::process::Command::new("cc");

            command.arg(config.cflags.clone());
            command.arg(config.ldflags.clone());
            command.arg(&format!("{manifest_dir}/../{test}.c"));
            command.arg(config.ldlibs.clone());
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

    for test in CAPI_BASE_TESTS.iter() {
        let _ = std::fs::remove_file(&format!("{manifest_dir}/{test}.obj"));
        let _ = std::fs::remove_file(&format!("{manifest_dir}/../{test}.exe"));
        let _ = std::fs::remove_file(&format!("{manifest_dir}/../{test}"));
    }
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
