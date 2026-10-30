//! Source-built B11 fixture embedding. Every guest uses the public browser facade.
#[cfg(not(target_arch = "wasm32"))]
fn main() {}

#[cfg(target_arch = "wasm32")]
mod wasm {
    use serde::Deserialize;
    use std::{
        io,
        sync::{Arc, Mutex},
        task::{Poll, Waker},
    };
    use wasixv2::{Credentials, DirectoryEntry, Engine, Net, NetConfig, ProcessConfig, browser};
    use wasixv2_host::{
        BrowserHost, ByteSink, Executor, Host,
        browser::{BrowserExecutor, block_on},
    };
    use wasixv2_kernel::HostClock;
    use wasixv2_vfs::{Cred, Ctx, LookupFlags, OpenHow};
    use wasm_bindgen::prelude::*;

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Fixture {
        name: String,
        args: Vec<String>,
        env: Vec<(String, String)>,
        stdin: Vec<u8>,
        cwd: Option<String>,
        user: Option<String>,
        program_name: Option<String>,
        mounts: Vec<Mount>,
        expected: Expected,
        #[serde(default)]
        console: bool,
        #[serde(default)]
        http: bool,
        #[serde(default)]
        network: Option<String>,
        #[serde(default)]
        relay_url: Option<String>,
        #[serde(default)]
        command_defaults: Vec<CommandDefaults>,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct CommandDefaults {
        name: String,
        argv0: Option<String>,
        args: Vec<String>,
        env: Vec<String>,
    }
    #[derive(Deserialize)]
    struct Mount {
        guest: String,
        entries: Vec<DirectoryEntry>,
    }
    #[derive(Deserialize)]
    struct Expected {
        files: Vec<ExpectedFile>,
    }
    #[derive(Deserialize)]
    struct ExpectedFile {
        path: String,
    }
    #[derive(Default)]
    struct Output {
        result: Option<io::Result<(Vec<u8>, Vec<u8>)>>,
        waker: Option<Waker>,
    }

    async fn collect(source: Arc<wasixv2_stdio_web::Source>) -> io::Result<Vec<u8>> {
        let mut result = Vec::new();
        let mut bytes = vec![0; 16384];
        loop {
            let n = source.read(&mut bytes).await?;
            if n == 0 {
                return Ok(result);
            }
            if result.len() + n > 16 * 1024 * 1024 {
                source.cancel();
                return Err(io::Error::other("fixture output exceeded 16 MiB"));
            }
            result.extend_from_slice(&bytes[..n]);
        }
    }

    fn credentials(value: Option<&str>) -> Result<Credentials, JsValue> {
        let Some(value) = value else {
            return Ok(Credentials::default());
        };
        let (uid, gid) = value.split_once(':').unwrap_or((value, value));
        let parse = |v: &str| {
            v.parse::<u32>()
                .map_err(|e| JsValue::from_str(&e.to_string()))
        };
        Ok(Credentials::new(parse(uid)?, parse(gid)?))
    }
    fn error(e: impl std::fmt::Display) -> JsValue {
        JsValue::from_str(&e.to_string())
    }

    /// Called only by the trusted coordinator on its dedicated root worker. Fetch
    /// completed on the UI/I/O event loop before the module bytes were dispatched.
    #[wasm_bindgen]
    pub fn run_fixture(metadata: &str, module_bytes: &[u8]) -> Result<String, JsValue> {
        let fixture: Fixture = serde_json::from_str(metadata).map_err(error)?;
        block_on(execute(fixture, module_bytes)).map_err(error)
    }

    async fn execute(fixture: Fixture, wasm: &[u8]) -> Result<String, String> {
        async fn inner(fixture: Fixture, wasm: &[u8]) -> Result<String, JsValue> {
            let executor = BrowserExecutor::new(64, 8).map_err(error)?;
            let host = Arc::new(BrowserHost::new(executor.clone()).map_err(error)?);
            let user = credentials(fixture.user.as_deref())?;
            let owner = (user.uid(), user.gid());
            let (session, stdio, streams) = browser::Session::new(65536).map_err(error)?;
            let mut net_config = NetConfig::host_default();
            net_config.uplink.as_mut().unwrap().v6 = Some((
                "fec0::15".parse().unwrap(),
                64,
                Some("fec0::2".parse().unwrap()),
            ));
            let (network, relay) = if let Some(profile) = &fixture.network {
                if profile == "absent" {
                    (Net::new(net_config.clone(), None), None)
                } else {
                    let caps = if profile == "classic" {
                        browser::Capabilities::classic(true)
                    } else {
                        browser::Capabilities::classic_fidelity(true)
                    };
                    let (net, relay) = browser::relay::connect(
                        fixture
                            .relay_url
                            .as_deref()
                            .ok_or_else(|| error("missing relay URL"))?,
                        net_config.clone(),
                        browser::NetworkLimits::default(),
                        caps,
                        &executor,
                    )
                    .map_err(error)?;
                    (net, Some(relay))
                }
            } else {
                (Net::new(NetConfig::default(), None), None)
            };
            let mut publications = Vec::new();
            for (key, value) in &fixture.env {
                if key == "PUBLISH_TCP" && fixture.network.as_deref() != Some("peer") {
                    publications.push(
                        network
                            .publish(
                                format!("127.0.0.1:{value}").parse().map_err(error)?,
                                "0.0.0.0:8080".parse().unwrap(),
                            )
                            .map_err(error)?,
                    );
                }
                if key == "PUBLISH_UDP" && fixture.network.as_deref() != Some("peer") {
                    publications.push(
                        network
                            .publish_udp(
                                format!("127.0.0.1:{value}").parse().map_err(error)?,
                                "0.0.0.0:8081".parse().unwrap(),
                            )
                            .map_err(error)?,
                    );
                }
            }
            let mut builder =
                browser::builder(Arc::new(host.services()), executor.clone(), network).user(user);
            if fixture.http {
                builder = builder.wasix1_http_provider(Arc::new(
                    wasixv2::wasix1_http::browser::FetchProvider::new(Default::default())
                        .map_err(error)?,
                ));
            }
            if fixture.console {
                builder = builder.console(
                    session.console(wasixv2_host::WindowSize {
                        rows: 25,
                        cols: 80,
                        ..Default::default()
                    }),
                    [true, true, false],
                );
            }
            let mut machine = builder.build();
            machine
                .set_wasix1_command_defaults(fixture.command_defaults.into_iter().map(|c| {
                    (
                        c.name,
                        wasixv2::CommandDefaults {
                            argv0: c.argv0,
                            args: c.args,
                            env: c.env,
                        },
                    )
                }))
                .map_err(error)?;
            for mount in fixture.mounts {
                let fs =
                    browser::directory(Arc::new(HostClock(host.clone())), 0o700, owner.0, owner.1)
                        .await
                        .map_err(|e| error(format!("mount {}: {e:?}", mount.guest)))?;
                wasixv2::populate_directory(fs.clone(), &mount.entries, owner.0, owner.1)
                    .await
                    .map_err(|e| error(format!("snapshot {}: {e:?}", mount.guest)))?;
                machine
                    .mount(&mount.guest, fs, false)
                    .await
                    .map_err(error)?;
            }
            if let Some(cwd) = fixture.cwd {
                machine.set_working_dir(cwd);
            }
            let machine = Arc::new(machine);
            let output = Arc::new(Mutex::new(Output::default()));
            let captured = output.clone();
            executor
                .services()
                .spawn_thread(Box::new(move || {
                    Box::pin(async move {
                        let input = async {
                            let mut offset = 0;
                            while offset < fixture.stdin.len() {
                                match streams.stdin.write(&fixture.stdin[offset..]).await {
                                    Ok(0) | Err(_) => break,
                                    Ok(n) => offset += n,
                                }
                            }
                            if !fixture.console {
                                streams.stdin.close();
                            }
                        };
                        let (_, out, err) = futures_util::future::join3(
                            input,
                            collect(streams.stdout),
                            collect(streams.stderr),
                        )
                        .await;
                        let result = out.and_then(|out| err.map(|err| (out, err)));
                        let waker = {
                            let mut state = captured.lock().unwrap();
                            state.result = Some(result);
                            state.waker.take()
                        };
                        if let Some(waker) = waker {
                            waker.wake();
                        }
                    })
                }))
                .map_err(error)?;
            let engine = Engine::default();
            let module = wasixv2::load(&engine, wasm).map_err(error)?;
            let peer = if fixture.network.as_deref() == Some("peer") {
                let (network, relay) = browser::relay::connect(
                    fixture.relay_url.as_deref().unwrap(),
                    net_config,
                    browser::NetworkLimits::default(),
                    browser::Capabilities::classic_fidelity(true),
                    &executor,
                )
                .map_err(error)?;
                let port = fixture
                    .env
                    .iter()
                    .find(|(key, _)| key == "PUBLISH_TCP")
                    .ok_or_else(|| error("missing peer publication"))?
                    .1
                    .clone();
                let publication = network
                    .publish(
                        format!("127.0.0.1:{port}").parse().map_err(error)?,
                        "0.0.0.0:8080".parse().unwrap(),
                    )
                    .map_err(error)?;
                let peer = Arc::new(
                    browser::builder(Arc::new(host.services()), executor.clone(), network).build(),
                );
                let mut env = peer.default_env();
                env.extend(
                    fixture
                        .env
                        .iter()
                        .map(|(key, value)| format!("{key}={value}")),
                );
                let run = browser::spawn(
                    peer.clone(),
                    &executor,
                    engine.clone(),
                    module.clone(),
                    ProcessConfig {
                        args: vec!["relay".into(), "peer-server".into()],
                        env,
                        stdio: wasixv2::Stdio::from_fns(|_| {}, |_| {}),
                    },
                )
                .await
                .map_err(error)?;
                Some((peer, run, relay, publication))
            } else {
                None
            };
            let mut env = machine.default_env();
            for (name, value) in fixture.env {
                env.retain(|entry| {
                    entry.split_once('=').map(|(key, _)| key) != Some(name.as_str())
                });
                env.push(format!("{name}={value}"));
            }
            let config = ProcessConfig {
                args: std::iter::once(fixture.program_name.unwrap_or(fixture.name))
                    .chain(fixture.args)
                    .collect(),
                env,
                stdio,
            };
            let run = browser::spawn(machine.clone(), &executor, engine, module, config)
                .await
                .map_err(error)?;
            let result = run.wait().await;
            // Wait for descendants too before EOF, so inherited output producers
            // cannot write after the final process result (kernel/exit.c:exit_files).
            machine.shutdown().await;
            if let Some((peer, run, relay, publication)) = peer {
                let result = run.wait().await.map_err(error)?;
                peer.shutdown().await;
                drop(run);
                drop(peer);
                drop(publication);
                relay.shutdown().await;
                if result != 0 {
                    return Err(error(format!("peer guest exited {result}")));
                }
            }
            session.finish().await.map_err(error)?;
            let (stdout, stderr) = std::future::poll_fn(|cx| {
                let mut state = output.lock().unwrap();
                state.waker = Some(cx.waker().clone());
                state.result.take().map_or(Poll::Pending, Poll::Ready)
            })
            .await
            .map_err(error)?;
            let exit = result.map_err(error)?;
            let mut files = Vec::new();
            for file in fixture.expected.files {
                let vfs = machine.vfs();
                let root = vfs.root();
                let cred = Cred::user(owner.0, owner.1);
                let cx = Ctx {
                    cred: &cred,
                    root: &root,
                    umask: 0,
                    task: 0,
                    leases: None,
                };
                let open = vfs
                    .open(
                        &cx,
                        &root,
                        file.path.as_bytes(),
                        &OpenHow {
                            flags: wasixv2_abi::OpenFlags::O_RDONLY,
                            mode: 0,
                            lookup: LookupFlags::follow(),
                        },
                    )
                    .await
                    .map_err(|e| error(format!("expected file {}: {e:?}", file.path)))?;
                let mut data = Vec::new();
                let mut bytes = vec![0; 16384];
                loop {
                    let n = open
                        .read_at(&mut bytes, data.len() as u64)
                        .await
                        .map_err(|e| error(format!("read expected file: {e:?}")))?;
                    if n == 0 {
                        break;
                    }
                    data.extend_from_slice(&bytes[..n]);
                    if data.len() > 16 * 1024 * 1024 {
                        return Err(error("expected file too large"));
                    }
                }
                files.push(serde_json::json!({"path":file.path,"data":data}));
            }
            drop(run);
            drop(machine);
            drop(session);
            drop(publications);
            if let Some(relay) = relay {
                relay.shutdown().await;
            }
            executor.close();
            serde_json::to_string(
                &serde_json::json!({"exitCode":exit,"stdout":stdout,"stderr":stderr,"files":files}),
            )
            .map_err(error)
        }
        inner(fixture, wasm)
            .await
            .map_err(|e| e.as_string().unwrap_or_else(|| format!("{e:?}")))
    }
    /// Performance baseline: guest-side monotonic time around synchronous syscalls.
    /// Linux kernel/sys.c:__do_sys_getpid and kernel/time/posix-timers.c:
    /// posix_get_monotonic_timespec define the unchanged operations being measured.
    #[wasm_bindgen]
    pub fn benchmark_smoke(iterations: u32) -> Result<String, JsValue> {
        if !(1..=1_000_000).contains(&iterations) {
            return Err(error("iterations must be between 1 and 1000000"));
        }
        let wasm = wat::parse_str(format!(
            r#"(module
                (import "wasixv2" "exit_group" (func $exit (param i32)))
                (import "wasixv2" "getpid" (func $getpid (result i32)))
                (import "wasixv2" "clock_gettime64" (func $clock (param i32 i32) (result i32)))
                (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
                (memory (export "memory") 2 16 shared)
                (func (export "_start") (local $left i32)
                    i32.const {iterations} local.set $left
                    i32.const 1 i32.const 1024 call $clock drop
                    (loop $again
                        call $getpid drop
                        local.get $left i32.const 1 i32.sub local.tee $left br_if $again)
                    i32.const 1 i32.const 1040 call $clock drop
                    i32.const 1 i32.const 1024 i32.const 32 call $write drop
                    i32.const 0 call $exit unreachable))"#
        ))
        .map_err(error)?;
        let fixture = Fixture {
            name: "syscall-baseline".into(),
            args: vec![],
            env: vec![],
            stdin: vec![],
            cwd: None,
            user: None,
            program_name: None,
            mounts: vec![],
            expected: Expected { files: vec![] },
            console: false,
            http: false,
            network: None,
            relay_url: None,
            command_defaults: vec![],
        };
        block_on(execute(fixture, &wasm)).map_err(error)
    }

    /// Browser-only regression: two kernels, shared provider identity, cached waits,
    /// immediate kill and busy-guest terminal input, using the public B8 facade.
    #[wasm_bindgen]
    pub fn lifecycle_smoke() -> Result<String, JsValue> {
        use futures_util::FutureExt;
        block_on(async {
            let executor = BrowserExecutor::new(64, 8).map_err(error)?;
            let host = Arc::new(BrowserHost::new(executor.clone()).map_err(error)?);
            let make = || {
                browser::builder(
                    Arc::new(host.services()),
                    executor.clone(),
                    Net::new(NetConfig::default(), None),
                )
                .build()
            };
            let first = Arc::new(make());
            let second = Arc::new(make());
            let shared = browser::directory(Arc::new(HostClock(host.clone())), 0o700, 1, 1)
                .await
                .map_err(|e| error(format!("{e:?}")))?;
            wasixv2::populate_directory(
                shared.clone(),
                &[
                    DirectoryEntry::Directory {
                        path: "".into(),
                        mode: 0o700,
                    },
                    DirectoryEntry::File {
                        path: "value".into(),
                        mode: 0o600,
                        data: b"old".to_vec(),
                    },
                ],
                1,
                1,
            )
            .await
            .map_err(|e| error(format!("{e:?}")))?;
            first
                .mount("/shared", shared.clone(), false)
                .await
                .map_err(error)?;
            second
                .mount("/shared", shared, false)
                .await
                .map_err(error)?;
            let engine = Engine::default();
            let writer=wasixv2::load(&engine,&wat::parse_str(r#"(module
                (import "wasixv2" "openat" (func $open (param i32 i32 i32 i32) (result i32)))
                (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
                (import "wasixv2" "exit_group" (func $exit (param i32)))
                (memory (export "memory") 1) (data (i32.const 16) "/shared/value\00")
                (data (i32.const 64) "new")
                (func (export "_start")
                  (drop (call $write (call $open (i32.const -100) (i32.const 16) (i32.const 1) (i32.const 0)) (i32.const 64) (i32.const 3)))
                  (call $exit (i32.const 0)) unreachable))"#).map_err(error)?).map_err(error)?;
            let invalid = wasixv2::load(
                &engine,
                &wat::parse_str(
                    r#"(module
                (import "wasix_32v1" "missing_browser_import" (func))
                (memory (export "memory") 1) (func (export "_start")))"#,
                )
                .map_err(error)?,
            )
            .map_err(error)?;
            let invalid_result = browser::spawn(
                first.clone(),
                &executor,
                engine.clone(),
                invalid,
                ProcessConfig {
                    args: vec!["invalid".into()],
                    env: vec![],
                    stdio: wasixv2::Stdio::from_fns(|_| {}, |_| {}),
                },
            )
            .await;
            assert!(
                matches!(invalid_result, Err(wasixv2::RunError::Imports(ref message)) if message.contains("missing_browser_import"))
            );
            let config = || ProcessConfig {
                args: vec!["smoke".into()],
                env: vec![],
                stdio: wasixv2::Stdio::from_fns(|_| {}, |_| {}),
            };
            let written =
                browser::spawn(first.clone(), &executor, engine.clone(), writer, config())
                    .await
                    .map_err(error)?;
            assert_eq!(written.wait().await.map_err(error)?, 0);
            {
                let root = second.vfs().root();
                let cred = Cred::user(1, 1);
                let cx = Ctx {
                    cred: &cred,
                    root: &root,
                    umask: 0,
                    task: 0,
                    leases: None,
                };
                let how = OpenHow {
                    flags: wasixv2_abi::OpenFlags::O_RDONLY,
                    mode: 0,
                    lookup: LookupFlags::follow(),
                };
                let file = second
                    .vfs()
                    .open(&cx, &root, b"/shared/value", &how)
                    .await
                    .map_err(|e| error(format!("{e:?}")))?;
                let mut bytes = [0; 3];
                assert_eq!(
                    file.read_at(&mut bytes, 0)
                        .await
                        .map_err(|e| error(format!("{e:?}")))?,
                    3
                );
                assert_eq!(&bytes, b"new");
            }
            {
                let root = first.vfs().root();
                let cred = Cred::user(1, 1);
                let cx = Ctx {
                    cred: &cred,
                    root: &root,
                    umask: 0,
                    task: 0,
                    leases: None,
                };
                first
                    .vfs()
                    .mkdir(&cx, &root, b"/tmp/private", 0o700)
                    .await
                    .map_err(|e| error(format!("{e:?}")))?;
                let other = second.vfs().root();
                let cx = Ctx { root: &other, ..cx };
                assert!(
                    second
                        .vfs()
                        .lookup(&cx, &other, b"/tmp/private", LookupFlags::follow())
                        .await
                        .is_err()
                );
            }
            drop(written);
            let busy = wasixv2::load(
                &engine,
                &wat::parse_str(
                    r#"(module
                (import "wasixv2" "getpid" (func $pid (result i32)))
                (memory (export "memory") 1)
                (func (export "_start") (drop (call $pid)) (loop $busy br $busy)))"#,
                )
                .map_err(error)?,
            )
            .map_err(error)?;
            let a = browser::spawn(
                first.clone(),
                &executor,
                engine.clone(),
                busy.clone(),
                config(),
            )
            .await
            .map_err(error)?;
            let b = browser::spawn(
                second.clone(),
                &executor,
                engine.clone(),
                busy.clone(),
                config(),
            )
            .await
            .map_err(error)?;
            a.signaller().signal(15).map_err(error)?;
            assert_eq!(a.wait().await.map_err(error)?, 143);
            assert!(b.wait().now_or_never().is_none());
            assert_eq!(b.kill().await.map_err(error)?, 137);
            let (one, two) = futures_util::future::join(a.wait(), a.wait()).await;
            assert_eq!(one.map_err(error)?, 143);
            assert_eq!(two.map_err(error)?, 143);
            drop(a);
            drop(b);
            drop(first);
            drop(second);
            // The raw terminal byte must interrupt real computing guest code.
            let (session, stdio, streams) = browser::Session::new(4096).map_err(error)?;
            let terminal = Arc::new(
                browser::builder(
                    Arc::new(host.services()),
                    executor.clone(),
                    Net::new(NetConfig::default(), None),
                )
                .console(
                    session.console(wasixv2_host::WindowSize {
                        rows: 24,
                        cols: 80,
                        ..Default::default()
                    }),
                    [true, true, false],
                )
                .build(),
            );
            let run = browser::spawn(
                terminal.clone(),
                &executor,
                engine,
                busy,
                ProcessConfig {
                    args: vec!["tty".into()],
                    env: vec![],
                    stdio,
                },
            )
            .await
            .map_err(error)?;
            host.sleep(std::time::Duration::from_millis(10)).await;
            assert!(run.wait().now_or_never().is_none());
            assert_eq!(streams.stdin.write(&[3]).await.map_err(error)?, 1);
            assert_eq!(run.wait().await.map_err(error)?, 130);
            terminal.shutdown().await;
            let (finished, out, err) = futures_util::future::join3(
                session.finish(),
                collect(streams.stdout),
                collect(streams.stderr),
            )
            .await;
            finished.map_err(error)?;
            out.map_err(error)?;
            err.map_err(error)?;
            drop(run);
            drop(terminal);
            drop(session);
            executor.close();
            // Larger than B7's queue capacity in both directions: the independent
            // service must make progress while the guest blocks on read/write.
            let echo=wat::parse_str(r#"(module
                (import "wasixv2" "read" (func $read (param i32 i32 i32) (result i32)))
                (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
                (import "wasixv2" "exit_group" (func $exit (param i32)))
                (memory (export "memory") 1)
                (func $all (param $fd i32) (param $n i32) (local $p i32) (local $w i32)
                  (loop $more (local.set $w (call $write (local.get $fd) (local.get $p) (local.get $n)))
                    (local.set $p (i32.add (local.get $p) (local.get $w)))
                    (local.set $n (i32.sub (local.get $n) (local.get $w)))
                    (br_if $more (local.get $n))))
                (func (export "_start") (local $n i32)
                  (block $done (loop $next
                    (local.set $n (call $read (i32.const 0) (i32.const 0) (i32.const 8192)))
                    (br_if $done (i32.eqz (local.get $n)))
                    (call $all (i32.const 1) (local.get $n))
                    (memory.fill (i32.const 0) (i32.const 9) (local.get $n))
                    (call $all (i32.const 2) (local.get $n)) br $next))
                  (call $exit (i32.const 0)) unreachable))"#).map_err(error)?;
            let result = execute(
                Fixture {
                    name: "backpressure".into(),
                    args: vec![],
                    env: vec![],
                    stdin: vec![7; 262144],
                    cwd: None,
                    user: None,
                    program_name: None,
                    mounts: vec![],
                    expected: Expected { files: vec![] },
                    console: false,
                    http: false,
                    network: None,
                    relay_url: None,
                    command_defaults: vec![],
                },
                &echo,
            )
            .await
            .map_err(error)?;
            let result: serde_json::Value = serde_json::from_str(&result).map_err(error)?;
            assert_eq!(result["exitCode"], 0);
            for (name, byte) in [("stdout", 7), ("stderr", 9)] {
                let bytes = result[name].as_array().unwrap();
                assert_eq!(bytes.len(), 262144);
                assert!(bytes.iter().all(|v| v.as_u64() == Some(byte)));
            }
            Ok(serde_json::json!({"exitCode":0,"stdout":[],"stderr":[],"files":[],"sharedDirectory":true,"machineIsolation":true,"signal":true,"kill":true,"wait":true,"busyTty":true,"backpressure":true}).to_string())
        })
    }
    #[wasm_bindgen]
    pub fn relay_smoke(url: &str) -> Result<String, JsValue> {
        block_on(async {
            let executor = BrowserExecutor::new(8, 8).map_err(error)?;
            let host = Arc::new(BrowserHost::new(executor.clone()).map_err(error)?);
            let (network, relay) = browser::relay::connect(
                url,
                NetConfig::host_default(),
                browser::NetworkLimits::default(),
                browser::Capabilities::classic(false),
                &executor,
            )
            .map_err(error)?;
            let machine = Arc::new(
                browser::builder(Arc::new(host.services()), executor.clone(), network).build(),
            );
            let engine = Engine::default();
            let module=wasixv2::load(&engine,&wat::parse_str(r#"(module
                (import "wasixv2" "socket" (func $socket (param i32 i32 i32) (result i32)))
                (import "wasixv2" "connect" (func $connect (param i32 i32 i32) (result i32)))
                (import "wasixv2" "read" (func $read (param i32 i32 i32) (result i32)))
                (import "wasixv2" "write" (func $write (param i32 i32 i32) (result i32)))
                (import "wasixv2" "exit_group" (func $exit (param i32)))
                (memory (export "memory") 1)
                (data (i32.const 16) "\02\00\1f\90\c0\00\02\01\00\00\00\00\00\00\00\00")
                (data (i32.const 48) "ping")
                (func $check (param $n i32) (result i32)
                  (if (i32.lt_s (local.get $n) (i32.const 0)) (then (call $exit (i32.sub (i32.const 0) (local.get $n))) unreachable)) local.get $n)
                (func (export "_start") (local $fd i32)
                  (local.set $fd (call $check (call $socket (i32.const 2) (i32.const 1) (i32.const 0))))
                  (drop (call $check (call $connect (local.get $fd) (i32.const 16) (i32.const 16))))
                  (drop (call $check (call $write (local.get $fd) (i32.const 48) (i32.const 4))))
                  (drop (call $check (call $write (i32.const 1) (i32.const 64) (call $check (call $read (local.get $fd) (i32.const 64) (i32.const 4))))))
                  (call $exit (i32.const 0)) unreachable))"#).map_err(error)?).map_err(error)?;
            let output = Arc::new(Mutex::new(Vec::new()));
            let captured = output.clone();
            let config = ProcessConfig {
                args: vec!["relay".into()],
                env: vec![],
                stdio: wasixv2::Stdio::from_fns(
                    move |bytes| captured.lock().unwrap().extend_from_slice(bytes),
                    |_| {},
                ),
            };
            let run = browser::spawn(machine.clone(), &executor, engine, module, config)
                .await
                .map_err(error)?;
            assert_eq!(run.wait().await.map_err(error)?, 0);
            assert_eq!(&*output.lock().unwrap(), b"pong");
            machine.shutdown().await;
            drop(run);
            drop(machine);
            relay.shutdown().await;
            drop(relay);
            executor.close();
            Ok(serde_json::json!({"exitCode":0,"stdout":[],"stderr":[],"files":[],"relayTcp":true}).to_string())
        })
    }
}
