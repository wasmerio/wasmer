use std::{
    fs::{read_dir, File, OpenOptions, ReadDir},
    future::Future,
    io::{self, Read, SeekFrom},
    path::{Path, PathBuf},
    pin::Pin,
    sync::{mpsc, Arc, Mutex},
    task::{Context, Poll},
};

use virtual_fs::{
    host_fs, mem_fs, passthru_fs, tmp_fs, union_fs, AsyncRead, AsyncSeek, AsyncWrite,
    AsyncWriteExt, FileSystem, Pipe, ReadBuf, RootFileSystemBuilder,
};
use wasmer::{FunctionEnv, Imports, Module, Store};
use wasmer_wasix::runtime::{
    module_cache::ModuleHash,
    task_manager::{tokio::TokioTaskManager, InlineWaker},
};
use wasmer_wasix::types::wasi::{Filesize, Timestamp};
use wasmer_wasix::{
    generate_import_object_from_env, get_wasi_version, FsError, PluggableRuntime, VirtualFile,
    WasiEnv, WasiEnvBuilder, WasiVersion,
};
use wast::parser::{self, Parse, ParseBuffer, Parser};

/// The kind of filesystem `WasiTest` is going to use.
#[derive(Debug)]
pub enum WasiFileSystemKind {
    /// Instruct the test runner to use `virtual_fs::host_fs`.
    Host,

    /// Instruct the test runner to use `virtual_fs::mem_fs`.
    InMemory,

    /// Instruct the test runner to use `virtual_fs::tmp_fs`
    Tmp,

    /// Instruct the test runner to use `virtual_fs::passtru_fs`
    PassthruMemory,

    /// Instruct the test runner to use `virtual_fs::union_fs<host_fs, mem_fs>`
    UnionHostMemory,

    /// Instruct the test runner to use the TempFs returned by `virtual_fs::builder::RootFileSystemBuilder`
    RootFileSystemBuilder,
}

/// Crate holding metadata parsed from the WASI WAST about the test to be run.
#[derive(Debug, Clone, Hash)]
pub struct WasiTest<'a> {
    wasm_path: &'a str,
    args: Vec<&'a str>,
    envs: Vec<(&'a str, &'a str)>,
    dirs: Vec<&'a str>,
    mapped_dirs: Vec<(&'a str, &'a str)>,
    temp_dirs: Vec<&'a str>,
    assert_return: Option<AssertReturn>,
    stdin: Option<Stdin<'a>>,
    assert_stdout: Option<AssertStdout<'a>>,
    assert_stderr: Option<AssertStderr<'a>>,
}

// TODO: add `test_fs` here to sandbox better
const BASE_TEST_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../wasi-wast/wasi/");

fn get_stdio_output(rx: &mpsc::Receiver<Vec<u8>>) -> anyhow::Result<String> {
    let mut stdio = Vec::new();
    while let Ok(mut buf) = rx.try_recv() {
        stdio.append(&mut buf);
    }
    let stdout_str = std::str::from_utf8(&stdio[..])?;
    #[cfg(target_os = "windows")]
    // normalize line endings
    return Ok(stdout_str.replace("\r\n", "\n"));

    #[cfg(not(target_os = "windows"))]
    return Ok(stdout_str.to_string());
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
    pub fn run(
        &self,
        mut store: &mut Store,
        base_path: &str,
        filesystem_kind: WasiFileSystemKind,
    ) -> anyhow::Result<bool> {
        use anyhow::Context;

        #[cfg(not(target_arch = "wasm32"))]
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        #[cfg(not(target_arch = "wasm32"))]
        let handle = runtime.handle().clone();
        #[cfg(not(target_arch = "wasm32"))]
        let _guard = handle.enter();
        #[cfg(not(target_arch = "wasm32"))]
        let mut rt = PluggableRuntime::new(Arc::new(TokioTaskManager::new(runtime)));
        #[cfg(target_arch = "wasm32")]
        let mut rt = PluggableRuntime::new(Arc::new(TokioTaskManager::default()));
        rt.set_engine(Some(store.engine().clone()));

        let mut pb = PathBuf::from(base_path);
        pb.push(self.wasm_path);
        let wasm_bytes = {
            let mut wasm_module = File::open(pb)?;
            let mut out = vec![];
            wasm_module.read_to_end(&mut out)?;
            out
        };
        let module_hash = ModuleHash::hash(&wasm_bytes);

        let module = Module::new(store, wasm_bytes)?;
        let (builder, _tempdirs, mut stdin_tx, stdout_rx, stderr_rx) =
            { InlineWaker::block_on(async { self.create_wasi_env(filesystem_kind).await }) }?;

        let (instance, _wasi_env) =
            builder
                .runtime(Arc::new(rt))
                .instantiate_ext(module, module_hash, store)?;

        let start = instance.exports.get_function("_start")?;

        if let Some(stdin) = &self.stdin {
            // let mut wasi_stdin = { wasi_env.data(store).stdin().unwrap().unwrap() };
            // Then we can write to it!
            let data = stdin.stream.to_string();
            InlineWaker::block_on(async move {
                stdin_tx.write_all(data.as_bytes()).await?;
                stdin_tx.shutdown().await?;

                Ok::<_, anyhow::Error>(())
            })?;
        } else {
            std::mem::drop(stdin_tx);
        }

        // TODO: handle errors here when the error fix gets shipped
        match start.call(&mut store, &[]) {
            Ok(_) => {}
            Err(e) => {
                let stdout_str = get_stdio_output(&stdout_rx)?;
                let stderr_str = get_stdio_output(&stderr_rx)?;
                Err(e).with_context(|| {
                    format!(
                        "failed to run WASI `_start` function: failed with stdout: \"{}\"\nstderr: \"{}\"",
                        stdout_str,
                        stderr_str,
                    )
                })?;
            }
        }

        if let Some(expected_stdout) = &self.assert_stdout {
            let stdout_str = get_stdio_output(&stdout_rx)?;
            dbg!(&expected_stdout, &stdout_str);
            assert_eq!(stdout_str, expected_stdout.expected);
        }

        if let Some(expected_stderr) = &self.assert_stderr {
            let stderr_str = get_stdio_output(&stderr_rx)?;
            assert_eq!(stderr_str, expected_stderr.expected);
        }

        Ok(true)
    }

    /// Create the wasi env with the given metadata.
    #[allow(clippy::type_complexity)]
    async fn create_wasi_env(
        &self,
        filesystem_kind: WasiFileSystemKind,
    ) -> anyhow::Result<(
        WasiEnvBuilder,
        Vec<tempfile::TempDir>,
        Pipe,
        mpsc::Receiver<Vec<u8>>,
        mpsc::Receiver<Vec<u8>>,
    )> {
        let mut builder = WasiEnv::builder(self.wasm_path);

        let (stdin_tx, stdin_rx) = Pipe::channel();
        builder.set_stdin(Box::new(stdin_rx));

        for (name, value) in &self.envs {
            builder.add_env(name, value);
        }

        let mut host_temp_dirs_to_not_drop = vec![];

        match filesystem_kind {
            WasiFileSystemKind::Host => {
                let fs = host_fs::FileSystem::default();

                for (alias, real_dir) in &self.mapped_dirs {
                    let mut dir = PathBuf::from(BASE_TEST_DIR);
                    dir.push(real_dir);
                    builder.add_map_dir(alias, dir)?;
                }

                // due to the structure of our code, all preopen dirs must be mapped now
                for dir in &self.dirs {
                    let mut new_dir = PathBuf::from(BASE_TEST_DIR);
                    new_dir.push(dir);
                    builder.add_map_dir(dir, new_dir)?;
                }

                for alias in &self.temp_dirs {
                    let temp_dir = tempfile::tempdir()?;
                    builder.add_map_dir(alias, temp_dir.path())?;
                    host_temp_dirs_to_not_drop.push(temp_dir);
                }

                builder.set_fs(Box::new(fs));
            }

            other => {
                let fs: Box<dyn FileSystem + Send + Sync> = match other {
                    WasiFileSystemKind::InMemory => Box::<mem_fs::FileSystem>::default(),
                    WasiFileSystemKind::Tmp => Box::<tmp_fs::TmpFileSystem>::default(),
                    WasiFileSystemKind::PassthruMemory => {
                        let fs = Box::<mem_fs::FileSystem>::default();
                        Box::new(passthru_fs::PassthruFileSystem::new(fs))
                    }
                    WasiFileSystemKind::RootFileSystemBuilder => {
                        Box::new(RootFileSystemBuilder::new().build())
                    }
                    WasiFileSystemKind::UnionHostMemory => {
                        let a = mem_fs::FileSystem::default();
                        let b = mem_fs::FileSystem::default();
                        let c = mem_fs::FileSystem::default();
                        let d = mem_fs::FileSystem::default();
                        let e = mem_fs::FileSystem::default();
                        let f = mem_fs::FileSystem::default();

                        let mut union = union_fs::UnionFileSystem::new();

                        union.mount("mem_fs", "/test_fs", false, Box::new(a), None);
                        union.mount("mem_fs_2", "/snapshot1", false, Box::new(b), None);
                        union.mount("mem_fs_3", "/tests", false, Box::new(c), None);
                        union.mount("mem_fs_4", "/nightly_2022_10_18", false, Box::new(d), None);
                        union.mount("mem_fs_5", "/unstable", false, Box::new(e), None);
                        union.mount("mem_fs_6", "/.tmp_wasmer_wast_0", false, Box::new(f), None);

                        Box::new(union)
                    }
                    _ => {
                        panic!("unexpected filesystem type {:?}", other);
                    }
                };

                let mut temp_dir_index: usize = 0;

                let root = PathBuf::from("/");

                map_host_fs_to_mem_fs(&*fs, read_dir(BASE_TEST_DIR)?, &root).await?;

                for (alias, real_dir) in &self.mapped_dirs {
                    let mut path = root.clone();
                    path.push(real_dir);
                    builder.add_map_dir(alias, path)?;
                }

                for dir in &self.dirs {
                    let mut new_dir = PathBuf::from("/");
                    new_dir.push(dir);

                    builder.add_map_dir(dir, new_dir)?;
                }

                for alias in &self.temp_dirs {
                    let temp_dir_name =
                        PathBuf::from(format!("/.tmp_wasmer_wast_{}", temp_dir_index));
                    fs.create_dir(temp_dir_name.as_path())?;
                    builder.add_map_dir(alias, temp_dir_name)?;
                    temp_dir_index += 1;
                }

                builder.set_fs(fs);
            }
        }

        let (stdout, stdout_rx) = OutputCapturerer::new();
        let (stderr, stderr_rx) = OutputCapturerer::new();
        let builder = builder
            .args(&self.args)
            // adding this causes some tests to fail. TODO: investigate this
            //.env("RUST_BACKTRACE", "1")
            .stdout(Box::new(stdout))
            .stderr(Box::new(stderr));

        Ok((
            builder,
            host_temp_dirs_to_not_drop,
            stdin_tx,
            stdout_rx,
            stderr_rx,
        ))
    }

    /// Get the correct [`WasiVersion`] from the Wasm [`Module`].
    fn get_version(&self, module: &Module) -> anyhow::Result<WasiVersion> {
        use anyhow::Context;
        let version = get_wasi_version(module, true)
            .with_context(|| "failed to detect a version of WASI from the module")?;
        Ok(version)
    }

    /// Get the correct WASI import object for the given module and set it up with the
    /// [`WasiEnv`].
    fn get_imports(
        &self,
        store: &mut Store,
        ctx: &FunctionEnv<WasiEnv>,
        module: &Module,
    ) -> anyhow::Result<Imports> {
        let version = self.get_version(module)?;
        Ok(generate_import_object_from_env(store, ctx, version))
    }
}

mod wasi_kw {
    wast::custom_keyword!(wasi_test);
    wast::custom_keyword!(envs);
    wast::custom_keyword!(args);
    wast::custom_keyword!(preopens);
    wast::custom_keyword!(map_dirs);
    wast::custom_keyword!(temp_dirs);
    wast::custom_keyword!(assert_return);
    wast::custom_keyword!(stdin);
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

            let temp_dirs = if parser.peek2::<wasi_kw::temp_dirs>() {
                parser.parens(|p| p.parse::<TempDirs>())?.temp_dirs
            } else {
                vec![]
            };

            let assert_return = if parser.peek2::<wasi_kw::assert_return>() {
                Some(parser.parens(|p| p.parse::<AssertReturn>())?)
            } else {
                None
            };

            let stdin = if parser.peek2::<wasi_kw::stdin>() {
                Some(parser.parens(|p| p.parse::<Stdin>())?)
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
                temp_dirs,
                assert_return,
                stdin,
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
            //debug_assert!(strs.next().is_none());
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
    map_dirs: Vec<(&'a str, &'a str)>,
}

impl<'a> Parse<'a> for MapDirs<'a> {
    fn parse(parser: Parser<'a>) -> parser::Result<Self> {
        let mut map_dirs = vec![];
        parser.parse::<wasi_kw::map_dirs>()?;

        while parser.peek::<&'a str>() {
            let res = parser.parse::<&'a str>()?;
            let mut iter = res.split(':');
            let dir = iter.next().unwrap();
            let alias = iter.next().unwrap();
            map_dirs.push((dir, alias));
        }
        Ok(Self { map_dirs })
    }
}

#[derive(Debug, Clone, Hash)]
struct TempDirs<'a> {
    temp_dirs: Vec<&'a str>,
}

impl<'a> Parse<'a> for TempDirs<'a> {
    fn parse(parser: Parser<'a>) -> parser::Result<Self> {
        let mut temp_dirs = vec![];
        parser.parse::<wasi_kw::temp_dirs>()?;

        while parser.peek::<&'a str>() {
            let alias = parser.parse::<&'a str>()?;
            temp_dirs.push(alias);
        }
        Ok(Self { temp_dirs })
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
struct Stdin<'a> {
    stream: &'a str,
}

impl<'a> Parse<'a> for Stdin<'a> {
    fn parse(parser: Parser<'a>) -> parser::Result<Self> {
        parser.parse::<wasi_kw::stdin>()?;
        Ok(Self {
            stream: parser.parse()?,
        })
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

    #[tokio::test]
    async fn test_parse() {
        let pb = wast::parser::ParseBuffer::new(
            r#"(wasi_test "my_wasm.wasm"
                    (envs "HELLO=WORLD" "RUST_BACKTRACE=1")
                    (args "hello" "world" "--help")
                    (preopens "." "src/io")
                    (assert_return (i64.const 0))
                    (stdin "This is another \"string\" inside a string!")
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
        assert_eq!(
            result.stdin.unwrap().stream,
            "This is another \"string\" inside a string!"
        );
        assert_eq!(result.assert_stderr.unwrap().expected, "");
    }
}

#[derive(Debug, Clone)]
struct OutputCapturerer {
    output: Arc<Mutex<mpsc::Sender<Vec<u8>>>>,
}

impl OutputCapturerer {
    fn new() -> (Self, mpsc::Receiver<Vec<u8>>) {
        let (tx, rx) = mpsc::channel();
        (
            Self {
                output: Arc::new(Mutex::new(tx)),
            },
            rx,
        )
    }
}

impl VirtualFile for OutputCapturerer {
    fn last_accessed(&self) -> Timestamp {
        0
    }
    fn last_modified(&self) -> Timestamp {
        0
    }
    fn created_time(&self) -> Timestamp {
        0
    }
    fn size(&self) -> u64 {
        0
    }
    fn set_len(&mut self, _new_size: Filesize) -> Result<(), FsError> {
        Ok(())
    }
    fn unlink(&mut self) -> Result<(), FsError> {
        Ok(())
    }
    fn poll_read_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(0))
    }
    fn poll_write_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(8192))
    }
}

impl AsyncSeek for OutputCapturerer {
    fn start_seek(self: Pin<&mut Self>, _position: SeekFrom) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "can not seek logging wrapper",
        ))
    }
    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        Poll::Ready(Err(io::Error::new(
            io::ErrorKind::Other,
            "can not seek logging wrapper",
        )))
    }
}

impl AsyncWrite for OutputCapturerer {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.output
            .lock()
            .unwrap()
            .send(buf.to_vec())
            .map_err(|err| io::Error::new(io::ErrorKind::BrokenPipe, err.to_string()))?;
        Poll::Ready(Ok(buf.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl AsyncRead for OutputCapturerer {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Poll::Ready(Err(io::Error::new(
            io::ErrorKind::Other,
            "can not read from logging wrapper",
        )))
    }
}

/// When using `virtual_fs::mem_fs`, we cannot rely on `BASE_TEST_DIR`
/// because the host filesystem cannot be used. Instead, we are
/// copying `BASE_TEST_DIR` to the `mem_fs`.
fn map_host_fs_to_mem_fs<'a>(
    fs: &'a dyn FileSystem,
    directory_reader: ReadDir,
    path_prefix: &'a Path,
) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + 'a>> {
    Box::pin(async move {
        for entry in directory_reader {
            let entry = entry?;
            let entry_type = entry.file_type()?;

            let path = path_prefix.join(entry.path().file_name().unwrap());

            if entry_type.is_dir() {
                fs.create_dir(&path)?;

                map_host_fs_to_mem_fs(fs, read_dir(entry.path())?, &path).await?
            } else if entry_type.is_file() {
                let mut host_file = OpenOptions::new().read(true).open(entry.path())?;
                let mut mem_file = fs
                    .new_open_options()
                    .create_new(true)
                    .write(true)
                    .open(path)?;
                let mut buffer = Vec::new();
                Read::read_to_end(&mut host_file, &mut buffer)?;
                mem_file.write_all(&buffer).await?;
            } else if entry_type.is_symlink() {
                //unimplemented!("`mem_fs` does not support symlink for the moment");
            }
        }

        Ok(())
    })
}
