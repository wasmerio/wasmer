extern crate byteorder;
extern crate structopt;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate base64;

fn main() {
    _impl::main();
}

#[cfg(not(unix))]
mod _impl {
    pub fn main() {
        panic!("wasmd is only supported on Unix-like systems.");
    }
}

#[cfg(unix)]
mod _impl {
    use std::thread;
    use std::sync::mpsc;
    use std::fs::File;
    use std::sync::Arc;
    use std::fmt::Debug;
    use std::os::unix::io::{AsRawFd, FromRawFd};
    use structopt::StructOpt;
    use serde::{Serialize, Deserialize};
    use wasmer::*;
    use wasmer_runtime::{
        Value,
        error::RuntimeError,
        Func,
    };
    use wasmer_runtime_core::{
        self,
        backend::{CompilerConfig, MemoryBoundCheckMode},
        loader::Instance as LoadedInstance,
    };
    #[cfg(feature = "backend:singlepass")]
    use wasmer_singlepass_backend::SinglePassCompiler;

    use std::io::prelude::*;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::collections::HashMap;
    use std::thread::Thread;

    use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

    #[derive(Debug, StructOpt)]
    #[structopt(name = "wasmd", about = "WebAssembly execution service.")]
    enum CLIOptions {
        #[structopt(name = "listen")]
        Listen(Listen),
    }

    #[derive(Debug, StructOpt)]
    struct Listen {
        #[structopt(long = "socket")]
        socket: String,
    }

    #[derive(Serialize, Deserialize, Debug)]
    struct InitMessage {
        #[serde(with = "base64_serde")]
        binary: Vec<u8>,
        env: HashMap<String, String>,
        args: Vec<String>,
        pre_opened_directories: Vec<String>,
    }

    #[derive(Serialize, Deserialize, Debug)]
    enum Operation {
        Run,
        #[serde(with = "base64_serde")]
        Stdin(Vec<u8>),
    }

    #[derive(Serialize, Deserialize, Debug)]
    enum Feedback {
        Terminate(u64),
        LoadError(String),
        RunError(String),
        #[serde(with = "base64_serde")]
        Stdout(Vec<u8>),
        #[serde(with = "base64_serde")]
        Stderr(Vec<u8>),
    }

    mod base64_serde {
        use serde::{Serializer, de, Deserialize, Deserializer};

        pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
            serializer.serialize_str(&base64::encode(bytes))
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
            let s = <&str>::deserialize(deserializer)?;
            base64::decode(s).map_err(de::Error::custom)
        }
    }

    fn error_to_string<E: Debug>(e: E) -> String {
        format!("{:?}", e)
    }

    fn pipe() -> Result<(File, File), String> {
        unsafe {
            let mut fds: [::libc::c_int; 2] = [0; 2];
            let ret = ::libc::pipe(fds.as_mut_ptr());
            if ret < 0 {
                Err("failed to create pipe".into())
            } else {
                Ok((
                    File::from_raw_fd(fds[0] as _),
                    File::from_raw_fd(fds[1] as _),
                ))
            }
        }
    }

    fn read_message<R: Read, T: for<'a> Deserialize<'a>>(stream: &mut R) -> Result<T, String> {
        let size = stream.read_u32::<LittleEndian>().map_err(error_to_string)?;
        if size > 1048576 * 16 {
            return Err("size too large".into());
        }
        let mut v = Vec::with_capacity(size as usize);
        unsafe {
            v.set_len(size as usize);
        }
        stream.read_exact(&mut v).map_err(error_to_string)?;
        Ok(::serde_json::from_slice(&v).map_err(error_to_string)?)
    }

    fn write_message<W: Write, T: Serialize>(stream: &mut W, x: &T) -> Result<(), String> {
        let v = ::serde_json::to_vec(x).map_err(error_to_string)?;
        stream.write_u32::<LittleEndian>(v.len() as _).map_err(error_to_string)?;
        stream.write_all(&v).map_err(error_to_string)?;
        Ok(())
    }

    fn handle_client(mut stream: UnixStream) -> Result<(), String> {
        // TODO: Switch user
        let init: Arc<InitMessage> = Arc::new(read_message(&mut stream)?);
        let module = Arc::new(webassembly::compile_with_config_with(
            &init.binary,
            CompilerConfig {
                symbol_map: None,
                ..Default::default()
            },
            &SinglePassCompiler::new(),
        ).map_err(error_to_string)?);

        if cfg!(feature = "wasi") && wasmer_wasi::is_wasi_module(&module) {}
        else {
            write_message(&mut stream, &Feedback::LoadError("WASI ABI validation failed".into()))?;
            return Err("WASI not enabled or not a WASI module".into());
        }

        let (stdin_r, mut stdin_w) = pipe()?;
        let (mut stdout_r, stdout_w) = pipe()?;
        let (mut stderr_r, stderr_w) = pipe()?;

        unsafe {
            assert!(::libc::dup2(stdin_r.as_raw_fd(), 0) == 0);
            assert!(::libc::dup2(stdout_w.as_raw_fd(), 1) == 1);
            //assert!(::libc::dup2(stderr_w.as_raw_fd(), 2) == 2);
        }
        
        let mut thread_handle: Option<::std::thread::JoinHandle<()>> = None;
        let (feedback_sender, feedback_receiver) = mpsc::channel();

        {
            let sender = feedback_sender.clone();
            ::std::thread::spawn(move || {
                let mut buf: Vec<u8> = vec![0; 16384];
                loop {
                    let n = stdout_r.read(&mut buf).unwrap();
                    if n == 0 {
                        break;
                    }
                    sender.send(Feedback::Stdout(buf[0..n].to_vec())).unwrap();
                }
            });
        }

        {
            let sender = feedback_sender.clone();
            ::std::thread::spawn(move || {
                let mut buf: Vec<u8> = vec![0; 16384];
                loop {
                    let n = stderr_r.read(&mut buf).unwrap();
                    if n == 0 {
                        break;
                    }
                    sender.send(Feedback::Stderr(buf[0..n].to_vec())).unwrap();
                }
            });
        }

        let mut stream_rev = stream.try_clone().unwrap();
        ::std::thread::spawn(move || {
            loop {
                let msg = feedback_receiver.recv().unwrap();
                if write_message(&mut stream_rev, &msg).is_err() {
                    ::std::process::exit(0);
                }
            }
        });

        loop {
            let op = read_message(&mut stream)?;
            match op {
                Operation::Run => {
                    if thread_handle.is_some() {
                        return Err("already running".into());
                    }
                    let feedback_sender = feedback_sender.clone();
                    let module = module.clone();
                    let init = init.clone();
                    
                    thread_handle = Some(::std::thread::spawn(move || {
                        let import_object = wasmer_wasi::generate_import_object(
                            init.args.iter().map(|x| x.as_bytes().to_vec()).collect(),
                            init.env.iter().map(|(k, v)| format!("{}={}", k, v).into_bytes()).collect(),
                            init.pre_opened_directories.clone(),
                        );
                        let instance = match module
                            .instantiate(&import_object) {
                                Ok(x) => x,
                                Err(e) => {
                                    feedback_sender.send(Feedback::RunError(format!("Can't instantiate module: {:?}", e))).unwrap();
                                    return;
                                }
                            };
                        let start: Func<(), ()> = match instance.func("_start") {
                            Ok(x) => x,
                            Err(_) => {
                                feedback_sender.send(Feedback::RunError("start function not found".into())).unwrap();
                                return;
                            }
                        };
                        let result = start.call();

                        let ret = match result {
                            Err(RuntimeError::Trap { msg }) => Feedback::RunError(format!("wasm trap occured: {}", msg)),
                            Err(RuntimeError::Error { data }) => {
                                if let Some(error_code) = data.downcast_ref::<wasmer_wasi::ExitCode>() {
                                    Feedback::Terminate(error_code.code as _)
                                } else {
                                    Feedback::RunError(format!("unknown error"))
                                }
                            },
                            Ok(_) => Feedback::Terminate(0),
                        };
                        feedback_sender.send(ret).unwrap();
                    }))
                },
                Operation::Stdin(v) => {
                    stdin_w.write_all(&v).map_err(error_to_string)?;
                },
            }
        }
    }

    fn run_listen(opts: Listen) {
        let listener = UnixListener::bind(&opts.socket).unwrap();
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    if unsafe { ::libc::fork() } == 0 {
                        match handle_client(stream) {
                            Ok(()) => {},
                            Err(e) => {
                                //::std::process::exit(1);
                                eprintln!("ERROR: {}", e);
                            }
                        }
                        ::std::process::exit(0);
                    }
                }
                Err(err) => {
                    panic!("{:?}", err);
                }
            }
        }
    }

    pub fn main() {
        let options = CLIOptions::from_args();
        match options {
            CLIOptions::Listen(listen) => {
                run_listen(listen);
            }
        }
    }
}