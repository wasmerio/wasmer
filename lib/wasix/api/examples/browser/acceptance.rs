//! R1/R3 test inputs over public embedding APIs, with guest-output handshakes.
use serde::Deserialize;
use std::{collections::BTreeMap, io, sync::Arc};
use wasixv2::{Machine, OwnedBuffer, Package};
use wasixv2_abi::{Errno, OpenFlags};
use wasixv2_host::{BrowserHost, ByteSink, WindowSize};
use wasixv2_kernel::HostClock;
use wasixv2_vfs::{Cred, Ctx, Filesystem, LookupFlags, OpenHow, RenameMode};
use wasixv2_vfs_web::{LocalProvider, MEMORY_CAPABILITIES, WebFs, protocol::*};
use wasm_bindgen::prelude::*;

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Scenario {
    pub package_version: Option<u8>,
    pub command: Option<String>,
    #[serde(default)]
    pub actions: Vec<Action>,
    #[serde(default)]
    pub live_vfs: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
    after: String,
    input: Option<Vec<u8>>,
    resize: Option<[u16; 4]>,
    #[serde(default)]
    hangup: bool,
    #[serde(default, rename = "closeInput")]
    close_input: bool,
    #[serde(default)]
    edit: bool,
}
fn error(e: impl std::fmt::Debug) -> JsValue {
    JsValue::from_str(&format!("acceptance: {e:?}"))
}

pub async fn install(
    machine: &mut Machine,
    wasm: &[u8],
    scenario: Option<&Scenario>,
) -> Result<Option<Package>, JsValue> {
    let Some(version) = scenario.and_then(|s| s.package_version) else {
        return Ok(None);
    };
    use webc::v3::{
        ChecksumAlgorithm, SignatureAlgorithm, Timestamps,
        write::{DirEntry, Directory, FileEntry, Writer},
    };
    let build = |manifest: serde_json::Value,
                 atom: &[u8],
                 volume: bool|
     -> Result<Package, JsValue> {
        let manifest = serde_json::from_value(manifest).map_err(error)?;
        let mut writer = Writer::new(ChecksumAlgorithm::Sha256)
            .write_manifest(&manifest)
            .map_err(error)?
            .write_atoms(BTreeMap::from([(
                "main".parse().unwrap(),
                FileEntry::borrowed(atom, Timestamps::default()),
            )]))
            .map_err(error)?;
        if volume {
            let sub = Directory::new(
                BTreeMap::from([(
                    "file".parse().unwrap(),
                    DirEntry::File(FileEntry::borrowed(b"inside", Timestamps::default())),
                )]),
                Timestamps::default(),
            );
            let dir = if version == 2 {
                sub
            } else {
                Directory::new(
                    BTreeMap::from([
                        ("sub".parse().unwrap(), DirEntry::Dir(sub)),
                        (
                            "outside".parse().unwrap(),
                            DirEntry::File(FileEntry::borrowed(b"hidden", Timestamps::default())),
                        ),
                    ]),
                    Timestamps::default(),
                )
            };
            writer.write_volume("files", dir).map_err(error)?;
        }
        let mut bytes =
            OwnedBuffer::from_bytes(writer.finish(SignatureAlgorithm::None).map_err(error)?);
        if version == 2 {
            bytes = webc::migration::v3_to_v2(bytes).map_err(error)?;
        } else if version != 3 {
            return Err(error("unsupported package version"));
        }
        Package::from_bytes(bytes).map_err(error)
    };
    let dependency = build(
        serde_json::json!({"commands":{"dependency":{"runner":"https://webc.org/runner/wasi","annotations":{"atom":{"name":"main"}}}}}),
        wasm,
        false,
    )?;
    assert!(Package::from_bytes(OwnedBuffer::from_bytes(&b"not a webc"[..])).is_err());
    let root = build(
        serde_json::json!({
            "entrypoint":"default", "use":{"dep":"https://example.invalid/dependency.webc"},
            "commands":{
                "default":{"runner":"https://webc.org/runner/wasi","annotations":{"atom":{"name":"main"},"wasi":{"atom":"main","exec-name":"configured","main-args":["default-arg"],"env":["DEFAULT=package","EXPLICIT=package"],"cwd":"/data"}}},
                "named":{"runner":"https://webc.org/runner/wasi","annotations":{"atom":{"name":"main","dependency":"dep"},"wasi":{"atom":"main","exec-name":"configured","main-args":["default-arg"],"env":["DEFAULT=package","EXPLICIT=package"],"cwd":"/data"}}}
            }, "package":{"fs":[{"volume_name":"files","host_path":"sub","mount_path":"/data"}]}
        }),
        wasm,
        true,
    )?;
    assert!(root.binary("named").is_err());
    let root = root.with_dependency("dep", Arc::new(dependency));
    let extra = build(
        serde_json::json!({"commands":{"extra":{"runner":"https://webc.org/runner/wasi","annotations":{"atom":{"name":"main"}}}}}),
        wasm,
        false,
    )?;
    // Missing dependency/command must fail before any guest can start.
    assert!(root.binary("missing").is_err());
    assert_eq!(root.commands().count(), 2);
    assert_eq!(root.binary("named").map_err(error)?.as_ref(), wasm);
    let collision_wasm = wat::parse_str(r#"(module (import "wasixv2" "exit_group" (func $exit (param i32))) (memory (export "memory") 1) (func (export "_start") (call $exit (i32.const 91)) unreachable))"#).map_err(error)?;
    let collision = build(
        serde_json::json!({"commands":{"default":{"runner":"https://webc.org/runner/wasi","annotations":{"atom":{"name":"main"}}}}}),
        &collision_wasm,
        false,
    )?;
    wasixv2::install_packages(machine, &[&root, &extra, &collision])
        .await
        .map_err(error)?;
    Ok(Some(root))
}

/// Inject a provider read failure, preserving the ordinary VFS path and errno.
/// Linux fs/read_write.c:vfs_read propagates the filesystem's read_iter failure.
struct FailingReads(Arc<LocalProvider>);
#[async_trait::async_trait(?Send)]
impl Provider for FailingReads {
    async fn call(&self, request: Request) -> wasixv2_vfs::FsResult<Reply> {
        if matches!(request, Request::Read(..)) {
            return Err(Errno::EIO);
        }
        if matches!(&request, Request::Lookup(_, name) if name == b"broken") {
            return Err(Errno::EIO);
        }
        self.0.call(request).await
    }
    fn release(&self, resource: Resource) {
        self.0.release(resource);
    }
    fn acl_version(&self) -> u64 {
        self.0.acl_version()
    }
}
pub async fn mount_live(machine: &Machine, host: Arc<BrowserHost>) -> Result<(), JsValue> {
    let clock = Arc::new(HostClock(host));
    let live = wasixv2::browser::directory(clock.clone(), 0o755, 1, 1)
        .await
        .map_err(error)?;
    wasixv2::populate_directory(
        live.clone(),
        &[
            wasixv2::DirectoryEntry::Directory {
                path: "".into(),
                mode: 0o755,
            },
            wasixv2::DirectoryEntry::File {
                path: "value".into(),
                mode: 0o600,
                data: b"old".to_vec(),
            },
            wasixv2::DirectoryEntry::File {
                path: "secret".into(),
                mode: 0o600,
                data: b"secret".to_vec(),
            },
        ],
        1,
        1,
    )
    .await
    .map_err(error)?;
    machine
        .mount("/live", live.clone(), false)
        .await
        .map_err(error)?;
    machine
        .mount("/readonly", live, true)
        .await
        .map_err(error)?;
    let fs = Arc::new(wasixv2_vfs_mem::MemFs::new(clock, 0o755, 1, 1));
    let info = fs.info();
    let provider = Arc::new(FailingReads(LocalProvider::new(fs)));
    let fault = WebFs::new(provider, info, MEMORY_CAPABILITIES)
        .await
        .map_err(error)?;
    wasixv2::populate_directory(
        fault.clone(),
        &[
            wasixv2::DirectoryEntry::Directory {
                path: "".into(),
                mode: 0o755,
            },
            wasixv2::DirectoryEntry::File {
                path: "value".into(),
                mode: 0o600,
                data: b"fault".to_vec(),
            },
        ],
        1,
        1,
    )
    .await
    .map_err(error)?;
    machine.mount("/fault", fault, false).await.map_err(error)?;
    Ok(())
}

async fn edit(machine: &Machine) -> wasixv2_vfs::FsResult<()> {
    let vfs = machine.vfs();
    let root = vfs.root();
    let cred = Cred::user(1, 1);
    let cx = Ctx {
        cred: &cred,
        root: &root,
        umask: 0,
        task: 0,
        leases: None,
    };
    let how = OpenHow {
        flags: OpenFlags::O_RDWR,
        mode: 0,
        lookup: LookupFlags::follow(),
    };
    // Same explicit credentials as a syscall. fs/namei.c:generic_permission/do_renameat2.
    let foreign = Cred::user(2, 2);
    let other = Ctx {
        cred: &foreign,
        ..cx
    };
    assert_eq!(
        vfs.open(&other, &root, b"/live/secret", &how).await.err(),
        Some(Errno::EACCES)
    );
    assert_eq!(
        vfs.open(&cx, &root, b"/readonly/value", &how).await.err(),
        Some(Errno::EROFS)
    );
    assert_eq!(
        vfs.lookup(&cx, &root, b"/fault/broken", LookupFlags::follow())
            .await
            .err(),
        Some(Errno::EIO)
    );
    let file = vfs.open(&cx, &root, b"/live/value", &how).await?;
    let mut bytes = [0; 5];
    assert_eq!(file.read_at(&mut bytes, 0).await?, 5);
    assert_eq!(&bytes, b"guest");
    assert_eq!(file.write_at(b"new", 0).await?, 3);
    vfs.rename(
        &cx,
        &root,
        b"/live/value",
        &root,
        b"/live/moved",
        RenameMode::Replace,
    )
    .await?;
    vfs.unlink(&cx, &root, b"/live/moved").await?;
    // An existing guest and embedder handle both keep the unlinked inode.
    assert_eq!(file.write_at(b"held", 0).await?, 4);
    let create = OpenHow {
        flags: OpenFlags::O_CREAT | OpenFlags::O_RDWR,
        mode: 0o600,
        lookup: LookupFlags::follow(),
    };
    let fresh = vfs.open(&cx, &root, b"/live/value", &create).await?;
    assert_eq!(fresh.write_at(b"fresh", 0).await?, 5);
    Ok(())
}

pub async fn collect_actions(
    source: Arc<wasixv2_stdio_web::Source>,
    input: Arc<wasixv2_stdio_web::Sink>,
    machine: Arc<Machine>,
    actions: Vec<Action>,
) -> io::Result<Vec<u8>> {
    let mut actions = actions.into_iter().peekable();
    let mut result = Vec::new();
    let mut bytes = vec![0; 16384];
    loop {
        let n = source.read(&mut bytes).await?;
        if n == 0 {
            if actions.peek().is_some() {
                return Err(io::Error::other("guest exited before acceptance action"));
            }
            return Ok(result);
        }
        if result.len() + n > 16 * 1024 * 1024 {
            return Err(io::Error::other("fixture output exceeded 16 MiB"));
        }
        result.extend_from_slice(&bytes[..n]);
        while actions.peek().is_some_and(|a| {
            result
                .windows(a.after.len())
                .any(|b| b == a.after.as_bytes())
        }) {
            let action = actions.next().unwrap();
            if action.edit {
                edit(&machine)
                    .await
                    .map_err(|e| io::Error::from_raw_os_error(e.0))?;
            }
            if let Some([rows, cols, xpixel, ypixel]) = action.resize {
                machine.signaller().console_resize(WindowSize {
                    rows,
                    cols,
                    xpixel,
                    ypixel,
                });
            }
            if action.close_input {
                input.close();
            }
            if action.hangup {
                machine.signaller().console_hangup();
            }
            if let Some(data) = action.input {
                let mut offset = 0;
                while offset < data.len() {
                    let n = input.write(&data[offset..]).await?;
                    if n == 0 {
                        return Err(io::Error::other("closed action input"));
                    }
                    offset += n;
                }
            }
        }
    }
}
