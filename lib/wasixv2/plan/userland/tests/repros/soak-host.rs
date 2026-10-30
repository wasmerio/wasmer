//! Repeated machines in ONE host process, following api/tests/machine.rs.
use std::{
    collections::HashMap,
    io::{self, Write},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use wasixv2::{
    Credentials, EmptySource, Engine, Image, LoadError, Loader, LocalFuture, MachineBuilder,
    ProcessConfig, Stdio, TokioHost,
};
use wasmer::Module;

#[derive(Default)]
struct Modules(Mutex<HashMap<Vec<u8>, Module>>);

impl Loader for Modules {
    fn load<'a>(
        &'a self,
        engine: &'a Engine,
        image: &'a Image,
    ) -> LocalFuture<'a, Result<(Engine, Module), LoadError>> {
        Box::pin(async move {
            let cached = self.0.lock().unwrap().get(image.bytes()).cloned();
            let module = match cached {
                Some(module) => module,
                None => {
                    let module = wasixv2::load(engine, image.bytes())?;
                    self.0
                        .lock()
                        .unwrap()
                        .insert(image.bytes().to_vec(), module.clone());
                    module
                }
            };
            Ok((engine.clone(), module))
        })
    }
}

struct Output;
impl wasixv2::ByteSink for Output {
    fn write<'a>(&'a self, bytes: &'a [u8]) -> LocalFuture<'a, io::Result<usize>> {
        Box::pin(async move {
            io::stdout().lock().write_all(bytes)?;
            Ok(bytes.len())
        })
    }
}

fn resources() -> (usize, usize, u64) {
    let count = |path| std::fs::read_dir(path).unwrap().count();
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let rss = status
        .lines()
        .find_map(|s| s.strip_prefix("VmRSS:"))
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .parse()
        .unwrap();
    (count("/proc/self/fd"), count("/proc/self/task"), rss)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let kit = PathBuf::from(args.next().expect("kit path"));
    let machines: usize = args.next().unwrap_or_else(|| "32".into()).parse().unwrap();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(3)
        .enable_all()
        .build()
        .unwrap();
    tokio::task::LocalSet::new().block_on(&runtime, async {
        let engine = Engine::default();
        let loader = Arc::new(Modules::default());
        for index in 1..=machines {
            let host = Arc::new(TokioHost::default());
            let host_weak = Arc::downgrade(&host);
            let machine = MachineBuilder::new(host)
                .user(Credentials::new(0, 0))
                .loader(loader.clone())
                .build();
            let net_weak = Arc::downgrade(machine.network());
            for (guest, path) in [
                ("/bin", kit.join("bin")),
                ("/usr/bin", kit.join("bin")),
                ("/tests", kit.join("tests")),
                (
                    "/terminfo",
                    PathBuf::from(std::env::var("HOME").unwrap())
                        .join("wasixv2-tools/userland-9/prefix/share/terminfo"),
                ),
            ] {
                machine.mount_host_dir(guest, &path, true).await.unwrap();
            }
            let config = ProcessConfig {
                args: ["bash", "/tests/soak-guest.sh", "1"]
                    .map(String::from)
                    .to_vec(),
                env: vec!["PATH=/bin:/usr/bin".into(), "HOME=/root".into()],
                stdio: Stdio {
                    stdin: Arc::new(EmptySource),
                    stdout: Arc::new(Output),
                    stderr: Arc::new(Output),
                },
            };
            let status = machine
                .run_file(&engine, "/bin/bash", config)
                .await
                .unwrap();
            assert_eq!(status, 0, "full mixed workflow in machine {index}");
            machine.shutdown().await;
            drop(machine);
            let deadline = Instant::now() + Duration::from_secs(5);
            while (host_weak.strong_count() != 0 || net_weak.strong_count() != 0)
                && Instant::now() < deadline
            {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            assert_eq!(
                net_weak.strong_count(),
                0,
                "network retained after machine {index}"
            );
            assert_eq!(
                host_weak.strong_count(),
                0,
                "host retained after machine {index}"
            );
            let (fds, threads, rss) = resources();
            println!("RETIRED {index} fds={fds} threads={threads} rss_kib={rss}");
        }
        println!("REPEATED MACHINES PASS {machines}");
    });
}
