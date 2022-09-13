#[derive(Debug)]
pub struct Config {
    pub wasmer_dir: String,
    pub root_dir: String,
    pub cflags: String,
    pub ldflags: String,
    pub ldlibs: String,
}

#[cfg(test)]
fn get_config() -> Config {
    Config {
        cflags: std::env::var("CFLAGS").unwrap(),
        wasmer_dir: std::env::var("WASMER_DIR").unwrap(),
        root_dir: std::env::var("ROOT_DIR").unwrap(),
        ldflags: std::env::var("LDFLAGS").unwrap(),
        ldlibs: std::env::var("LDLIBS").unwrap(),
    }
}

/*
CAPI_BASE_TESTS = \
	wasm-c-api/example/callback	wasm-c-api/example/global	wasm-c-api/example/hello \
	wasm-c-api/example/memory	wasm-c-api/example/reflect	wasm-c-api/example/serialize \
	wasm-c-api/example/start	wasm-c-api/example/trap		wasm-c-api/example/multi

CAPI_BASE_TESTS_NOT_WORKING = \
	wasm-c-api/example/finalize	wasm-c-api/example/hostref	wasm-c-api/example/threads \
	wasm-c-api/example/table

ALL = $(CAPI_BASE_TESTS)
*/

// Runs all the tests that are working in the /c directory
#[test]
fn test_ok() {
    // let compiler = "cc" CFLAGS LDFLAGS LDLIBS
    // for example in root_dir.join("").c { compiler.compile(...) }

    // target command on linux / mac:

    // cc -g -IC:/Users/felix/Development/wasmer/lib/c-api/tests/ 
    // -IC:/Users/felix/Development/wasmer/package/include  
    // -Wl,-rpath,C:/Users/felix/Development/wasmer/package/lib  
    // wasm-c-api/example/callback.c  
    // -LC:/Users/felix/Development/wasmer/package/lib 
    // -lwasmer -o wasm-c-api/example/callback
 
    println!("config: {:#?}", get_config());
}