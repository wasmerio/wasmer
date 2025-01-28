use std::{path::Path, sync::Arc};

use anyhow::Context;
use once_cell::sync::OnceCell;
use sha2::Digest;
use virtual_fs::FileSystem;
use wasmer_config::package::{PackageHash, PackageId, PackageSource};
use wasmer_package::package::Package;
use webc::compat::SharedBytes;
use webc::Container;

use crate::{
    runners::MappedDirectory,
    runtime::resolver::{PackageInfo, ResolveError},
    Runtime,
};
use wasmer_types::ModuleHash;

#[derive(derive_more::Debug, Clone)]
pub struct BinaryPackageCommand {
    name: String,
    metadata: webc::metadata::Command,
    #[debug(ignore)]
    pub(crate) atom: SharedBytes,
    hash: ModuleHash,
}

impl BinaryPackageCommand {
    pub fn new(
        name: String,
        metadata: webc::metadata::Command,
        atom: SharedBytes,
        hash: ModuleHash,
    ) -> Self {
        Self {
            name,
            metadata,
            atom,
            hash,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn metadata(&self) -> &webc::metadata::Command {
        &self.metadata
    }

    /// Get a reference to this [`BinaryPackageCommand`]'s atom as a cheap
    /// clone of the internal OwnedBuffer.
    pub fn atom(&self) -> SharedBytes {
        self.atom.clone()
    }

    pub fn hash(&self) -> &ModuleHash {
        &self.hash
    }
}

/// A WebAssembly package that has been loaded into memory.
#[derive(Debug, Clone)]
pub struct BinaryPackage {
    pub id: PackageId,
    /// Includes the ids of all the packages in the tree
    pub package_ids: Vec<PackageId>,

    pub when_cached: Option<u128>,
    /// The name of the [`BinaryPackageCommand`] which is this package's
    /// entrypoint.
    pub entrypoint_cmd: Option<String>,
    pub hash: OnceCell<ModuleHash>,
    pub webc_fs: Arc<dyn FileSystem + Send + Sync>,
    pub commands: Vec<BinaryPackageCommand>,
    pub uses: Vec<String>,
    pub file_system_memory_footprint: u64,

    pub additional_host_mapped_directories: Vec<MappedDirectory>,
}

impl BinaryPackage {
    #[tracing::instrument(level = "debug", skip_all)]
    pub async fn from_dir(
        dir: &Path,
        rt: &(dyn Runtime + Send + Sync),
    ) -> Result<Self, anyhow::Error> {
        let source = rt.source();

        // since each package must be in its own directory, hash of the `dir` should provide a good enough
        // unique identifier for the package
        let hash = sha2::Sha256::digest(dir.display().to_string().as_bytes()).into();
        let id = PackageId::Hash(PackageHash::from_sha256_bytes(hash));

        let manifest_path = dir.join("wasmer.toml");
        let webc = Package::from_manifest(&manifest_path)?;
        let container = Container::from(webc);
        let manifest = container.manifest();

        let root = PackageInfo::from_manifest(id, manifest, container.version())?;
        let root_id = root.id.clone();

        let resolution = crate::runtime::resolver::resolve(&root_id, &root, &*source).await?;
        let mut pkg = rt
            .package_loader()
            .load_package_tree(&container, &resolution, true)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        // HACK: webc has no way to return its deserialized manifest to us, so we need to do it again here
        // We already read and parsed the manifest once, so it'll succeed again. Unwrapping is safe at this point.
        let wasmer_toml = std::fs::read_to_string(&manifest_path).unwrap();
        let wasmer_toml: wasmer_config::package::Manifest = toml::from_str(&wasmer_toml).unwrap();
        pkg.additional_host_mapped_directories.extend(
            wasmer_toml
                .fs
                .into_iter()
                .map(|(guest, host)| {
                    anyhow::Ok(MappedDirectory {
                        host: dir.join(host).canonicalize()?,
                        guest,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?
                .into_iter(),
        );

        Ok(pkg)
    }

    /// Load a [`webc::Container`] and all its dependencies into a
    /// [`BinaryPackage`].
    #[tracing::instrument(level = "debug", skip_all)]
    pub async fn from_webc(
        container: &Container,
        rt: &(dyn Runtime + Send + Sync),
    ) -> Result<Self, anyhow::Error> {
        let source = rt.source();

        let manifest = container.manifest();
        let id = PackageInfo::package_id_from_manifest(manifest)?
            .or_else(|| {
                container
                    .webc_hash()
                    .map(|hash| PackageId::Hash(PackageHash::from_sha256_bytes(hash)))
            })
            .ok_or_else(|| anyhow::Error::msg("webc file did not provide its hash"))?;

        let root = PackageInfo::from_manifest(id, manifest, container.version())?;
        let root_id = root.id.clone();

        let resolution = crate::runtime::resolver::resolve(&root_id, &root, &*source).await?;
        let pkg = rt
            .package_loader()
            .load_package_tree(container, &resolution, false)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        Ok(pkg)
    }

    /// Load a [`BinaryPackage`] and all its dependencies from a registry.
    #[tracing::instrument(level = "debug", skip_all)]
    pub async fn from_registry(
        specifier: &PackageSource,
        runtime: &(dyn Runtime + Send + Sync),
    ) -> Result<Self, anyhow::Error> {
        let source = runtime.source();
        let root_summary =
            source
                .latest(specifier)
                .await
                .map_err(|error| ResolveError::Registry {
                    package: specifier.clone(),
                    error,
                })?;
        let root = runtime.package_loader().load(&root_summary).await?;
        let id = root_summary.package_id();

        let resolution = crate::runtime::resolver::resolve(&id, &root_summary.pkg, &source)
            .await
            .context("Dependency resolution failed")?;
        let pkg = runtime
            .package_loader()
            .load_package_tree(&root, &resolution, false)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        Ok(pkg)
    }

    pub fn get_command(&self, name: &str) -> Option<&BinaryPackageCommand> {
        self.commands.iter().find(|cmd| cmd.name() == name)
    }

    /// Resolve the entrypoint command name to a [`BinaryPackageCommand`].
    pub fn get_entrypoint_command(&self) -> Option<&BinaryPackageCommand> {
        self.entrypoint_cmd
            .as_deref()
            .and_then(|name| self.get_command(name))
    }

    /// Get the bytes for the entrypoint command.
    #[deprecated(
        note = "Use BinaryPackage::get_entrypoint_command instead",
        since = "0.22.0"
    )]
    pub fn entrypoint_bytes(&self) -> Option<SharedBytes> {
        self.get_entrypoint_command().map(|entry| entry.atom())
    }

    /// Get a hash for this binary package.
    ///
    /// Usually the hash of the entrypoint.
    pub fn hash(&self) -> ModuleHash {
        *self.hash.get_or_init(|| {
            if let Some(cmd) = self.get_entrypoint_command() {
                cmd.hash
            } else {
                ModuleHash::xxhash(self.id.to_string())
            }
        })
    }

    pub fn infer_entrypoint(&self) -> Result<&str, anyhow::Error> {
        if let Some(entrypoint) = self.entrypoint_cmd.as_deref() {
            return Ok(entrypoint);
        }

        match self.commands.as_slice() {
            [] => anyhow::bail!("The package doesn't contain any executable commands"),
            [one] => Ok(one.name()),
            [..] => {
                let mut commands: Vec<_> = self.commands.iter().map(|cmd| cmd.name()).collect();
                commands.sort();
                anyhow::bail!(
                    "Unable to determine the package's entrypoint. Please choose one of {:?}",
                    commands,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use sha2::Digest;
    use tempfile::TempDir;
    use virtual_fs::AsyncReadExt;
    use wasmer_package::utils::from_disk;

    use crate::{
        runtime::{package_loader::BuiltinPackageLoader, task_manager::VirtualTaskManager},
        PluggableRuntime,
    };

    use super::*;

    fn task_manager() -> Arc<dyn VirtualTaskManager + Send + Sync> {
        cfg_if::cfg_if! {
            if #[cfg(feature = "sys-thread")] {
                Arc::new(crate::runtime::task_manager::tokio::TokioTaskManager::new(tokio::runtime::Handle::current()))
            } else {
                unimplemented!("Unable to get the task manager")
            }
        }
    }

    #[tokio::test]
    #[cfg_attr(
        not(feature = "sys-thread"),
        ignore = "The tokio task manager isn't available on this platform"
    )]
    async fn fs_table_can_map_directories_to_different_names() {
        let temp = TempDir::new().unwrap();
        let wasmer_toml = r#"
            [package]
            name = "some/package"
            version = "0.0.0"
            description = "a dummy package"

            [fs]
            "/public" = "./out"
        "#;
        let manifest = temp.path().join("wasmer.toml");
        std::fs::write(&manifest, wasmer_toml).unwrap();
        let out = temp.path().join("out");
        std::fs::create_dir_all(&out).unwrap();
        let file_txt = "Hello, World!";
        std::fs::write(out.join("file.txt"), file_txt).unwrap();
        let tasks = task_manager();
        let mut runtime = PluggableRuntime::new(tasks);
        runtime.set_package_loader(
            BuiltinPackageLoader::new()
                .with_shared_http_client(runtime.http_client().unwrap().clone()),
        );

        let pkg = Package::from_manifest(&manifest).unwrap();
        let data = pkg.serialize().unwrap();
        let webc_path = temp.path().join("package.webc");
        std::fs::write(&webc_path, data).unwrap();

        let pkg = BinaryPackage::from_webc(&from_disk(&webc_path).unwrap(), &runtime)
            .await
            .unwrap();

        // We should have mapped "./out/file.txt" on the host to
        // "/public/file.txt" on the guest.
        let mut f = pkg
            .webc_fs
            .new_open_options()
            .read(true)
            .open("/public/file.txt")
            .unwrap();
        let mut buffer = String::new();
        f.read_to_string(&mut buffer).await.unwrap();
        assert_eq!(buffer, file_txt);
    }

    #[tokio::test]
    #[cfg_attr(
        not(feature = "sys-thread"),
        ignore = "The tokio task manager isn't available on this platform"
    )]
    async fn commands_use_the_atom_signature() {
        let temp = TempDir::new().unwrap();
        let wasmer_toml = r#"
            [package]
            name = "some/package"
            version = "0.0.0"
            description = "a dummy package"

            [[module]]
            name = "foo"
            source = "foo.wasm"
            abi = "wasi"
            
            [[command]]
            name = "cmd"
            module = "foo"     
        "#;
        let manifest = temp.path().join("wasmer.toml");
        std::fs::write(&manifest, wasmer_toml).unwrap();

        let atom_path = temp.path().join("foo.wasm");
        std::fs::write(&atom_path, b"").unwrap();

        let webc: Container = Package::from_manifest(&manifest).unwrap().into();

        let tasks = task_manager();
        let mut runtime = PluggableRuntime::new(tasks);
        runtime.set_package_loader(
            BuiltinPackageLoader::new()
                .with_shared_http_client(runtime.http_client().unwrap().clone()),
        );

        let pkg = BinaryPackage::from_dir(temp.path(), &runtime)
            .await
            .unwrap();

        assert_eq!(pkg.commands.len(), 1);
        let command = pkg.get_command("cmd").unwrap();
        let atom_sha256_hash: [u8; 32] = sha2::Sha256::digest(webc.get_atom("foo").unwrap()).into();
        let module_hash = ModuleHash::sha256_from_bytes(atom_sha256_hash);
        assert_eq!(command.hash(), &module_hash);
    }
}
