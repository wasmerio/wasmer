//! Byte-backed package assembly. Acquisition, credentials and persistent caches belong
//! to the embedder; execution remains ordinary VFS exec (`fs/exec.c:kernel_execve`).
use crate::{Layer, Machine, OwnedBuffer};
use std::{
    collections::{BTreeMap, HashSet},
    io,
    sync::Arc,
};
use webc::{Version, metadata::annotations::Wasi, v3::read::OwnedReader};

/// A WEBC with dependencies supplied as bytes rather than fetched by the runtime.
/// Dependency aliases are those in the WEBC manifest's `use` map.
pub struct Package {
    reader: OwnedReader,
    dependencies: BTreeMap<String, Arc<Package>>,
}

impl Package {
    pub fn from_bytes(bytes: OwnedBuffer) -> io::Result<Self> {
        let bytes = match webc::detect(bytes.as_ref()).map_err(io::Error::other)? {
            Version::V3 => bytes,
            Version::V2 => webc::migration::v2_to_v3(bytes).map_err(io::Error::other)?,
            version => {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    format!("unsupported WEBC {version}"),
                ));
            }
        };
        Ok(Self {
            reader: OwnedReader::parse(bytes).map_err(io::Error::other)?,
            dependencies: BTreeMap::new(),
        })
    }

    pub fn with_dependency(mut self, alias: impl Into<String>, package: Arc<Package>) -> Self {
        self.dependencies.insert(alias.into(), package);
        self
    }

    pub fn entrypoint(&self) -> Option<&str> {
        self.reader.manifest().entrypoint.as_deref()
    }
    pub fn commands(&self) -> impl Iterator<Item = &str> {
        self.reader.manifest().commands.keys().map(String::as_str)
    }

    /// The command's module bytes, retaining the package buffer for a registry/cache
    /// consumer. No native filesystem or engine-specific serialization is needed.
    pub fn binary(&self, command: &str) -> io::Result<OwnedBuffer> {
        let command = self
            .reader
            .manifest()
            .commands
            .get(command)
            .ok_or_else(|| invalid("unknown package command"))?;
        let atom = command
            .atom()
            .map_err(io::Error::other)?
            .ok_or_else(|| invalid("command has no atom"))?;
        let source = self.dependency(atom.dependency.as_deref())?;
        source
            .reader
            .get_atom(&atom.name)
            .map(|(_, bytes)| bytes.clone())
            .ok_or_else(|| invalid("command atom is missing"))
    }

    pub fn annotation(&self, command: &str) -> io::Result<Option<Wasi>> {
        self.reader
            .manifest()
            .commands
            .get(command)
            .ok_or_else(|| invalid("unknown package command"))?
            .wasi()
            .map_err(io::Error::other)
    }

    /// Prepare a named/default command after installing this package tree.
    /// `config.args` contains caller arguments without argv[0]; command defaults
    /// precede them, explicit environment values and an explicit cwd win. This is
    /// container launch configuration; guest execve keeps its existing ABI policy.
    pub fn prepare_command(
        &self,
        machine: &mut Machine,
        command: Option<&str>,
        config: &mut crate::ProcessConfig,
    ) -> io::Result<String> {
        let name = command
            .or_else(|| self.entrypoint())
            .ok_or_else(|| invalid("package has no entrypoint"))?;
        self.binary(name)?;
        let annotation = self.annotation(name)?;
        let mut args = vec![
            annotation
                .as_ref()
                .and_then(|a| a.exec_name.clone())
                .unwrap_or_else(|| name.to_owned()),
        ];
        if let Some(annotation) = annotation {
            args.extend(annotation.main_args.unwrap_or_default());
            for entry in annotation.env.unwrap_or_default() {
                let key = entry.split_once('=').map_or(entry.as_str(), |(key, _)| key);
                if !config.env.iter().any(|value| {
                    value.split_once('=').map_or(value.as_str(), |(key, _)| key) == key
                }) {
                    config.env.push(entry);
                }
            }
            if machine.working_dir.is_none()
                && let Some(cwd) = annotation.cwd
            {
                machine.set_working_dir(
                    cwd.to_str()
                        .ok_or_else(|| invalid("package cwd is not UTF-8"))?,
                );
            }
        }
        args.append(&mut config.args);
        config.args = args;
        Ok(format!("/bin/{name}"))
    }

    fn dependency(&self, alias: Option<&str>) -> io::Result<&Package> {
        match alias {
            None => Ok(self),
            Some(alias) => self
                .dependencies
                .get(alias)
                .map(Arc::as_ref)
                .ok_or_else(|| invalid(&format!("missing supplied dependency {alias}"))),
        }
    }

    /// Installs the package and its supplied dependencies before mounts/processes.
    /// Root commands win, then dependencies in alias order. Dependencies' volumes
    /// are mounted first; the root package overlays them. Additional `uses` packages
    /// can be installed together by `install_packages`.
    pub async fn install(&self, machine: &mut Machine) -> io::Result<()> {
        install_packages(machine, &[self]).await
    }
}

/// Install a root package followed by additional injected packages. The first
/// command of a name wins; all file access remains under the Machine's VFS root.
pub async fn install_packages(machine: &mut Machine, packages: &[&Package]) -> io::Result<()> {
    let mut ordered = Vec::new();
    let mut seen = HashSet::new();
    let mut pending = packages.iter().rev().copied().collect::<Vec<_>>();
    while let Some(package) = pending.pop() {
        if !seen.insert(package as *const Package) {
            continue;
        }
        ordered.push(package);
        for (alias, _) in &package.reader.manifest().use_map {
            package.dependency(Some(alias))?;
        }
        pending.extend(package.dependencies.values().rev().map(Arc::as_ref));
    }
    let mut commands = Vec::new();
    #[cfg(feature = "wasix1")]
    let mut defaults = Vec::new();
    let mut names = HashSet::new();
    for package in &ordered {
        for name in package.commands() {
            if names.insert(name.to_owned()) {
                commands.push((name.to_owned(), package.binary(name)?));
                #[cfg(feature = "wasix1")]
                if let Some(wasi) = package.annotation(name)? {
                    defaults.push((
                        name.to_owned(),
                        crate::CommandDefaults {
                            argv0: wasi.exec_name,
                            args: wasi.main_args.unwrap_or_default(),
                            env: wasi.env.unwrap_or_default(),
                        },
                    ));
                }
            }
        }
    }
    let mut root_layers: Vec<Layer> = Vec::new();
    let mut mounts = Vec::new();
    for package in ordered.iter().rev() {
        if let Some(mappings) = package
            .reader
            .manifest()
            .filesystem()
            .map_err(io::Error::other)?
        {
            for mapping in mappings {
                let source = package.dependency(mapping.from.as_deref())?;
                if !mapping.mount_path.starts_with('/') {
                    return Err(invalid("package mount must be absolute"));
                }
                let mut layer = machine.package_layer(&source.reader, &mapping.volume_name)?;
                if let Some(path) = mapping
                    .host_path
                    .as_deref()
                    .filter(|p| !matches!(*p, "" | "/"))
                {
                    let vfs = wasixv2_vfs::Vfs::new(layer.0.clone());
                    let root = vfs.root();
                    let cred = wasixv2_vfs::Cred::root();
                    let cx = wasixv2_vfs::Ctx {
                        cred: &cred,
                        root: &root,
                        umask: 0,
                        task: 0,
                        leases: None,
                    };
                    let node = vfs
                        .lookup(
                            &cx,
                            &root,
                            path.as_bytes(),
                            wasixv2_vfs::LookupFlags {
                                in_root: true,
                                directory: true,
                                ..wasixv2_vfs::LookupFlags::follow()
                            },
                        )
                        .await
                        .map_err(crate::errno_to_io)?;
                    layer = Layer(Arc::new(VolumeView {
                        fs: layer.0,
                        root: node.dentry.node().clone(),
                    }));
                }
                if mapping.mount_path == "/" {
                    root_layers.push(layer);
                } else {
                    mounts.push((mapping.mount_path, layer));
                }
            }
        }
    }
    root_layers.extend(machine.commands_layer(commands)?);
    machine.mount_root_layers(root_layers).await?;
    #[cfg(feature = "wasix1")]
    machine.set_wasix1_command_defaults(defaults)?;
    // Parents before children; equal-depth entries preserve package precedence.
    mounts.sort_by_key(|(path, _)| path.split('/').filter(|c| !c.is_empty()).count());
    for (path, layer) in mounts {
        machine.mount_layer(&path, &layer, true).await?;
    }
    Ok(())
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_owned())
}

#[cfg(all(test, feature = "tokio", not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use webc::v3::{
        ChecksumAlgorithm, SignatureAlgorithm, Timestamps,
        write::{FileEntry, Writer},
    };
    fn bytes(code: i32) -> OwnedBuffer {
        let module = wat::parse_str(format!(r#"(module (import "wasixv2" "exit_group" (func $exit (param i32))) (memory (export "memory") 1) (func (export "_start") (call $exit (i32.const {code})) unreachable))"#)).unwrap();
        let manifest = serde_json::from_value(serde_json::json!({"entrypoint":"hello","commands":{"hello":{"runner":"https://webc.org/runner/wasi","annotations":{"atom":{"name":"main"}}}}})).unwrap();
        let writer = Writer::new(ChecksumAlgorithm::Sha256)
            .write_manifest(&manifest)
            .unwrap()
            .write_atoms(BTreeMap::from([(
                "main".parse().unwrap(),
                FileEntry::borrowed(&module, Timestamps::default()),
            )]))
            .unwrap();
        OwnedBuffer::from_bytes(writer.finish(SignatureAlgorithm::None).unwrap())
    }
    #[tokio::test]
    async fn byte_backed_packages_install_commands_without_native_sources() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let root = Package::from_bytes(bytes(23)).unwrap();
                let injected = Package::from_bytes(bytes(99)).unwrap();
                assert_eq!(root.entrypoint(), Some("hello"));
                assert!(root.binary("absent").is_err());
                let mut machine = Machine::new();
                install_packages(&mut machine, &[&root, &injected])
                    .await
                    .unwrap();
                let config = crate::ProcessConfig {
                    args: vec!["hello".into()],
                    env: vec![],
                    stdio: crate::Stdio::from_fns(|_| {}, |_| {}),
                };
                assert_eq!(
                    machine
                        .run_file(&crate::Engine::default(), "/bin/hello", config)
                        .await
                        .unwrap(),
                    23
                );
                machine.shutdown().await;
            })
            .await;
    }
    #[tokio::test]
    async fn supplied_dependency_atoms_survive_the_package_sources() {
        tokio::task::LocalSet::new().run_until(async {
            let manifest=serde_json::from_value(serde_json::json!({"entrypoint":"alias","use":{"dep":"https://example.invalid/dep.webc"},"commands":{"alias":{"runner":"https://webc.org/runner/wasi","annotations":{"atom":{"name":"main","dependency":"dep"}}}}})).unwrap();
            let writer=Writer::new(ChecksumAlgorithm::Sha256).write_manifest(&manifest).unwrap().write_atoms(BTreeMap::new()).unwrap();
            let unresolved=Package::from_bytes(OwnedBuffer::from_bytes(writer.finish(SignatureAlgorithm::None).unwrap())).unwrap();
            assert!(unresolved.binary("alias").is_err());
            let package=unresolved.with_dependency("dep",Arc::new(Package::from_bytes(bytes(77)).unwrap()));
            let mut machine=Machine::new();package.install(&mut machine).await.unwrap();drop(package);
            let config=crate::ProcessConfig{args:vec!["alias".into()],env:vec![],stdio:crate::Stdio::from_fns(|_|{},|_|{})};
            assert_eq!(machine.run_file(&crate::Engine::default(),"/bin/alias",config).await.unwrap(),77);
        }).await;
    }
    #[test]
    fn initial_command_configuration_preserves_explicit_values() {
        let mut package = Package::from_bytes(bytes(0)).unwrap();
        // Rebuild the manifest with the same source atom and launch annotations.
        let manifest=serde_json::from_value(serde_json::json!({"entrypoint":"hello","commands":{"hello":{"runner":"https://webc.org/runner/wasi","annotations":{"atom":{"name":"main"},"wasi":{"atom":"main","exec-name":"package-name","main-args":["default"],"env":["FOO=package","BAR=package"],"cwd":"/tmp"}}}}})).unwrap();
        let atom = package.binary("hello").unwrap();
        let writer = Writer::new(ChecksumAlgorithm::Sha256)
            .write_manifest(&manifest)
            .unwrap()
            .write_atoms(BTreeMap::from([(
                "main".parse().unwrap(),
                FileEntry::borrowed(&atom, Timestamps::default()),
            )]))
            .unwrap();
        package = Package::from_bytes(OwnedBuffer::from_bytes(
            writer.finish(SignatureAlgorithm::None).unwrap(),
        ))
        .unwrap();
        let mut machine = Machine::new();
        let mut config = crate::ProcessConfig {
            args: vec!["caller".into()],
            env: vec!["FOO=caller".into()],
            stdio: crate::Stdio::from_fns(|_| {}, |_| {}),
        };
        assert_eq!(
            package
                .prepare_command(&mut machine, None, &mut config)
                .unwrap(),
            "/bin/hello"
        );
        assert_eq!(config.args, ["package-name", "default", "caller"]);
        assert_eq!(config.env, ["FOO=caller", "BAR=package"]);
        assert_eq!(machine.working_dir.as_deref(), Some("/tmp"));
        machine.set_working_dir("/home/user");
        assert!(
            package
                .prepare_command(&mut machine, Some("hello"), &mut config)
                .is_ok()
        );
        assert_eq!(machine.working_dir.as_deref(), Some("/home/user"));
    }
    #[tokio::test]
    async fn package_subdirectory_is_a_vfs_mount_root() {
        use wasixv2_vfs::{Cred, Ctx, LookupFlags, OpenHow};
        use webc::v3::write::{DirEntry, Directory};
        let manifest = serde_json::from_value(serde_json::json!({"package":{"fs":[{"volume_name":"files","host_path":"sub","mount_path":"/data"}]}})).unwrap();
        let sub = Directory::new(
            BTreeMap::from([(
                "file".parse().unwrap(),
                DirEntry::File(FileEntry::borrowed(b"inside", Timestamps::default())),
            )]),
            Timestamps::default(),
        );
        let volume = Directory::new(
            BTreeMap::from([
                ("sub".parse().unwrap(), DirEntry::Dir(sub)),
                (
                    "outside".parse().unwrap(),
                    DirEntry::File(FileEntry::borrowed(b"hidden", Timestamps::default())),
                ),
            ]),
            Timestamps::default(),
        );
        let mut writer = Writer::new(ChecksumAlgorithm::Sha256)
            .write_manifest(&manifest)
            .unwrap()
            .write_atoms(BTreeMap::new())
            .unwrap();
        writer.write_volume("files", volume).unwrap();
        let package = Package::from_bytes(OwnedBuffer::from_bytes(
            writer.finish(SignatureAlgorithm::None).unwrap(),
        ))
        .unwrap();
        let mut machine = Machine::new();
        package.install(&mut machine).await.unwrap();
        let vfs = machine.vfs();
        let root = vfs.root();
        let cred = Cred::root();
        let cx = Ctx {
            cred: &cred,
            root: &root,
            umask: 0,
            task: 0,
            leases: None,
        };
        let file = vfs
            .open(
                &cx,
                &root,
                b"/data/file",
                &OpenHow {
                    flags: wasixv2_abi::OpenFlags::O_RDONLY,
                    mode: 0,
                    lookup: LookupFlags::follow(),
                },
            )
            .await
            .unwrap();
        let mut bytes = [0; 6];
        assert_eq!(file.read_at(&mut bytes, 0).await.unwrap(), 6);
        assert_eq!(&bytes, b"inside");
        for path in [b"/data/outside".as_slice(), b"/data/../outside"] {
            assert_eq!(
                vfs.lookup(&cx, &root, path, LookupFlags::follow())
                    .await
                    .err(),
                Some(wasixv2_abi::Errno::ENOENT)
            );
        }
    }
    #[tokio::test]
    async fn version_two_sources_migrate_from_bytes() {
        let original = bytes(4);
        let v2 = webc::migration::v3_to_v2(original.clone()).unwrap();
        let package = Package::from_bytes(v2).unwrap();
        assert_eq!(package.entrypoint(), Some("hello"));
        assert_eq!(
            package.binary("hello").unwrap(),
            Package::from_bytes(original)
                .unwrap()
                .binary("hello")
                .unwrap()
        );
        assert!(Package::from_bytes(OwnedBuffer::from_bytes(&b"bad"[..])).is_err());
    }
}

// Linux fs/namespace.c:clone_mnt retains a superblock while selecting a directory
// as a bind mount root. Package storage and readonly capabilities remain unchanged.
struct VolumeView {
    fs: Arc<dyn crate::Filesystem>,
    root: Arc<dyn wasixv2_vfs::Node>,
}
#[async_trait::async_trait(?Send)]
impl crate::Filesystem for VolumeView {
    fn limits_file_size(&self) -> bool {
        self.fs.limits_file_size()
    }
    fn info(&self) -> wasixv2_vfs::FsInfo {
        self.fs.info()
    }
    fn root(&self) -> Arc<dyn wasixv2_vfs::Node> {
        self.root.clone()
    }
    async fn statfs(&self) -> wasixv2_vfs::FsResult<wasixv2_vfs::StatFs> {
        self.fs.statfs().await
    }
    async fn sync(&self) -> wasixv2_vfs::FsResult<()> {
        self.fs.sync().await
    }
}
