use anyhow::Context;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;
use wasmer::{ImportObject, Instance, Memory, Module, Store};
use wasmer_wasi::{
    generate_import_object_from_env, get_wasi_version, WasiEnv, WasiState, WasiVersion,
};
use wast::parser::{self, Parse, ParseBuffer, Parser};

/// Crate holding metadata parsed from the WASI WAST about the test to be run.
#[derive(Debug, Clone, Hash)]
pub struct WasiTest<'a> {
    wasm_path: &'a str,
    args: Vec<&'a str>,
    envs: Vec<(&'a str, &'a str)>,
    dirs: Vec<&'a str>,
    mapped_dirs: Vec<&'a str>,
    assert_return: Option<AssertReturn>,
    assert_stdout: Option<AssertStdout<'a>>,
    assert_stderr: Option<AssertStderr<'a>>,
}

#[allow(dead_code)]
impl<'a> WasiTest<'a> {
    /// Turn a WASI WAST string into a list of tokens.
    pub fn lex_string(wast: &'a str) -> parser::Result<ParseBuffer<'a>> {
        ParseBuffer::new(wast)
    }

    /// Turn a WASI WAST list of tokens into a `WasiTest` struct.
    pub fn parse_tokens(tokens: &'a ParseBuffer<'a>) -> parser::Result<Self> {
        parser::parse(tokens)
    }

    /// Execute the WASI test and assert.
    pub fn run(&self, store: &Store, base_path: &str) -> anyhow::Result<bool> {
        let mut pb = PathBuf::from(base_path);
        pb.push(self.wasm_path);
        let wasm_bytes = {
            let mut wasm_module = File::open(pb)?;
            let mut out = vec![];
            wasm_module.read_to_end(&mut out)?;
            out
        };
        let module = Module::new(&store, &wasm_bytes)?;
        let mut env = self.create_wasi_env()?;
        let imports = self.get_imports(store, &module, env.clone())?;
        let instance = Instance::new(&module, &imports)?;
        let memory: &Memory = instance.exports.get("memory")?;
        // TODO:
        env.set_memory(Arc::new(memory.clone()));

        let start = instance.exports.get_function("_start")?;
        start
            .call(&[])
            .with_context(|| "failed to run WASI `_start` function")?;
        Ok(true)
    }

    /// Create the wasi env with the given metadata.
    fn create_wasi_env(&self) -> anyhow::Result<WasiEnv> {
        let mut builder = WasiState::new(self.wasm_path);
        for (name, value) in &self.envs {
            builder.env(name, value);
        }
        // TODO: implement map dirs
        /*
        // TODO: check the order
        for (alias, real_dir) in &self.mapped_dirs {
            builder.map_dir(alias, real_dir);
        }*/

        let out = builder
            .args(&self.args)
            .preopen_dirs(&self.dirs)?
            // TODO: capture stdout and stderr
            // can be done with a custom file, inserted here
            .finalize()?;
        Ok(out)
    }

    /// Get the correct [`WasiVersion`] from the Wasm [`Module`].
    fn get_version(&self, module: &Module) -> anyhow::Result<WasiVersion> {
        let version = get_wasi_version(module, true)
            .with_context(|| "failed to detect a version of WASI from the module")?;
        Ok(version)
    }

    /// Get the correct WASI import object for the given module and set it up with the
    /// [`WasiEnv`].
    fn get_imports(
        &self,
        store: &Store,
        module: &Module,
        env: WasiEnv,
    ) -> anyhow::Result<ImportObject> {
        let version = self.get_version(module)?;
        Ok(generate_import_object_from_env(store, env, version))
    }
}

mod wasi_kw {
    wast::custom_keyword!(wasi_test);
    wast::custom_keyword!(envs);
    wast::custom_keyword!(args);
    wast::custom_keyword!(preopens);
    wast::custom_keyword!(map_dirs);
    wast::custom_keyword!(assert_return);
    wast::custom_keyword!(assert_stdout);
    wast::custom_keyword!(assert_stderr);
    wast::custom_keyword!(fake_i64_const = "i64.const");
}

impl<'a> Parse<'a> for WasiTest<'a> {
    fn parse(parser: Parser<'a>) -> parser::Result<Self> {
        parser.parens(|parser| {
            parser.parse::<wasi_kw::wasi_test>()?;
            // TODO: improve error message here
            let wasm_path = parser.parse::<&'a str>()?;

            // TODO: allow these to come in any order
            let envs = if parser.peek2::<wasi_kw::envs>() {
                parser.parens(|p| p.parse::<Envs>())?.envs
            } else {
                vec![]
            };

            let args = if parser.peek2::<wasi_kw::args>() {
                parser.parens(|p| p.parse::<Args>())?.args
            } else {
                vec![]
            };

            let dirs = if parser.peek2::<wasi_kw::preopens>() {
                parser.parens(|p| p.parse::<Preopens>())?.preopens
            } else {
                vec![]
            };

            let mapped_dirs = if parser.peek2::<wasi_kw::map_dirs>() {
                parser.parens(|p| p.parse::<MapDirs>())?.map_dirs
            } else {
                vec![]
            };

            let assert_return = if parser.peek2::<wasi_kw::assert_return>() {
                Some(parser.parens(|p| p.parse::<AssertReturn>())?)
            } else {
                None
            };

            let assert_stdout = if parser.peek2::<wasi_kw::assert_stdout>() {
                Some(parser.parens(|p| p.parse::<AssertStdout>())?)
            } else {
                None
            };

            let assert_stderr = if parser.peek2::<wasi_kw::assert_stderr>() {
                Some(parser.parens(|p| p.parse::<AssertStderr>())?)
            } else {
                None
            };

            Ok(Self {
                wasm_path,
                args,
                envs,
                dirs,
                mapped_dirs,
                assert_return,
                assert_stdout,
                assert_stderr,
            })
        })
    }
}

#[derive(Debug, Clone, Hash)]
struct Envs<'a> {
    envs: Vec<(&'a str, &'a str)>,
}

impl<'a> Parse<'a> for Envs<'a> {
    fn parse(parser: Parser<'a>) -> parser::Result<Self> {
        let mut envs = vec![];
        parser.parse::<wasi_kw::envs>()?;

        while parser.peek::<&'a str>() {
            let res = parser.parse::<&'a str>()?;
            let mut strs = res.split('=');
            let first = strs.next().unwrap();
            let second = strs.next().unwrap();
            debug_assert!(strs.next().is_none());
            envs.push((first, second));
        }
        Ok(Self { envs })
    }
}

#[derive(Debug, Clone, Hash)]
struct Args<'a> {
    args: Vec<&'a str>,
}

impl<'a> Parse<'a> for Args<'a> {
    fn parse(parser: Parser<'a>) -> parser::Result<Self> {
        let mut args = vec![];
        parser.parse::<wasi_kw::args>()?;

        while parser.peek::<&'a str>() {
            let res = parser.parse::<&'a str>()?;
            args.push(res);
        }
        Ok(Self { args })
    }
}

#[derive(Debug, Clone, Hash)]
struct Preopens<'a> {
    preopens: Vec<&'a str>,
}

impl<'a> Parse<'a> for Preopens<'a> {
    fn parse(parser: Parser<'a>) -> parser::Result<Self> {
        let mut preopens = vec![];
        parser.parse::<wasi_kw::preopens>()?;

        while parser.peek::<&'a str>() {
            let res = parser.parse::<&'a str>()?;
            preopens.push(res);
        }
        Ok(Self { preopens })
    }
}

#[derive(Debug, Clone, Hash)]
struct MapDirs<'a> {
    map_dirs: Vec<&'a str>,
}

impl<'a> Parse<'a> for MapDirs<'a> {
    fn parse(parser: Parser<'a>) -> parser::Result<Self> {
        let mut map_dirs = vec![];
        parser.parse::<wasi_kw::map_dirs>()?;

        while parser.peek::<&'a str>() {
            let res = parser.parse::<&'a str>()?;
            map_dirs.push(res);
        }
        Ok(Self { map_dirs })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct AssertReturn {
    return_value: i64,
}

impl<'a> Parse<'a> for AssertReturn {
    fn parse(parser: Parser<'a>) -> parser::Result<Self> {
        parser.parse::<wasi_kw::assert_return>()?;
        let return_value = parser.parens(|p| {
            p.parse::<wasi_kw::fake_i64_const>()?;
            p.parse::<i64>()
        })?;
        Ok(Self { return_value })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct AssertStdout<'a> {
    expected: &'a str,
}

impl<'a> Parse<'a> for AssertStdout<'a> {
    fn parse(parser: Parser<'a>) -> parser::Result<Self> {
        parser.parse::<wasi_kw::assert_stdout>()?;
        Ok(Self {
            expected: parser.parse()?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct AssertStderr<'a> {
    expected: &'a str,
}

impl<'a> Parse<'a> for AssertStderr<'a> {
    fn parse(parser: Parser<'a>) -> parser::Result<Self> {
        parser.parse::<wasi_kw::assert_stderr>()?;
        Ok(Self {
            expected: parser.parse()?,
        })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_parse() {
        let pb = wast::parser::ParseBuffer::new(
            r#"(wasi_test "my_wasm.wasm"
                    (envs "HELLO=WORLD" "RUST_BACKTRACE=1")
                    (args "hello" "world" "--help")
                    (preopens "." "src/io")
                    (assert_return (i64.const 0))
                    (assert_stdout "This is a \"string\" inside a string!")
                    (assert_stderr "")
)"#,
        )
        .unwrap();
        let result = wast::parser::parse::<WasiTest>(&pb).unwrap();

        assert_eq!(result.args, vec!["hello", "world", "--help"]);
        assert_eq!(
            result.envs,
            vec![("HELLO", "WORLD"), ("RUST_BACKTRACE", "1")]
        );
        assert_eq!(result.dirs, vec![".", "src/io"]);
        assert_eq!(result.assert_return.unwrap().return_value, 0);
        assert_eq!(
            result.assert_stdout.unwrap().expected,
            "This is a \"string\" inside a string!"
        );
        assert_eq!(result.assert_stderr.unwrap().expected, "");
    }
}
