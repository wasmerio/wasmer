#![cfg(not(target_family = "wasm"))]

use std::{collections::HashMap, sync::Arc};

use anyhow::{Context, Error};
use tempfile::TempDir;
use url::Url;
use wasmer_config::package::{PackageId, PackageSource};
use wasmer_package::package::Package;
use wasmer_wasix::{
    PluggableRuntime,
    bin_factory::BinaryPackage,
    runtime::{
        package_loader::{PackageLoader, load_package_tree},
        resolver::{
            DistributionInfo, InMemorySource, PackageInfo, PackageSummary, Resolution, WebcHash,
        },
        task_manager::VirtualTaskManager,
    },
};
use webc::Container;

#[derive(Debug)]
struct InMemoryPackageLoader {
    containers: HashMap<PackageId, Container>,
}

#[async_trait::async_trait]
impl PackageLoader for InMemoryPackageLoader {
    async fn load(&self, summary: &PackageSummary) -> Result<Container, Error> {
        let id = summary.package_id();
        self.containers
            .get(&id)
            .cloned()
            .with_context(|| format!("No container found for \"{id}\""))
    }

    async fn load_package_tree(
        &self,
        root: &Container,
        resolution: &Resolution,
        root_is_local_dir: bool,
    ) -> Result<BinaryPackage, Error> {
        load_package_tree(root, self, resolution, root_is_local_dir).await
    }
}

fn task_manager() -> Arc<dyn VirtualTaskManager + Send + Sync> {
    cfg_if::cfg_if! {
        if #[cfg(feature = "sys-thread")] {
            Arc::new(
                wasmer_wasix::runtime::task_manager::tokio::TokioTaskManager::new(
                    tokio::runtime::Handle::current(),
                ),
            )
        } else {
            unimplemented!("Unable to get the task manager")
        }
    }
}

fn write_package(dir: &std::path::Path, manifest: &str, wasm_files: &[&str]) {
    std::fs::write(dir.join("wasmer.toml"), manifest).unwrap();
    for file in wasm_files {
        std::fs::write(dir.join(file), b"").unwrap();
    }
}

fn summary_for(container: &Container, id: PackageId) -> PackageSummary {
    let manifest = container.manifest();

    PackageSummary {
        pkg: PackageInfo::from_manifest(id.clone(), manifest, container.version()).unwrap(),
        dist: DistributionInfo {
            webc: Url::parse(&format!("https://example.invalid/{id}.webc")).unwrap(),
            webc_sha256: WebcHash::sha256(id.to_string()),
        },
    }
}

#[tokio::test]
#[cfg_attr(
    not(feature = "sys-thread"),
    ignore = "The tokio task manager isn't available on this platform"
)]
async fn command_tracks_declaring_package_and_atom_provider_package() {
    let temp = TempDir::new().unwrap();
    let dep_dir = temp.path().join("dep");
    let root_dir = temp.path().join("root");
    std::fs::create_dir_all(&dep_dir).unwrap();
    std::fs::create_dir_all(&root_dir).unwrap();

    write_package(
        &dep_dir,
        r#"
            [package]
            name = "wasmer/bash"
            version = "1.0.0"
            description = "bash package"

            [[module]]
            name = "bash"
            source = "bash.wasm"
            abi = "wasi"

            [[command]]
            name = "bash"
            module = "bash"
        "#,
        &["bash.wasm"],
    );

    write_package(
        &root_dir,
        r#"
            [package]
            name = "acme/app"
            version = "1.0.0"
            description = "root package"

            [dependencies]
            "wasmer/bash" = "1.0.0"

            [[command]]
            name = "after_deploy"
            module = "wasmer/bash:bash"
        "#,
        &[],
    );

    let dep_container: Container = Package::from_manifest(dep_dir.join("wasmer.toml"))
        .unwrap()
        .into();
    let root_container: Container = Package::from_manifest(root_dir.join("wasmer.toml"))
        .unwrap()
        .into();

    let dep_id = PackageId::new_named("wasmer/bash", "1.0.0".parse().unwrap());
    let root_id = PackageId::new_named("acme/app", "1.0.0".parse().unwrap());

    let dep_summary = summary_for(&dep_container, dep_id.clone());
    let root_summary = summary_for(&root_container, root_id.clone());

    let mut source = InMemorySource::new();
    source.add(dep_summary);
    source.add(root_summary);

    let loader = InMemoryPackageLoader {
        containers: HashMap::from([
            (dep_id.clone(), dep_container),
            (root_id.clone(), root_container),
        ]),
    };

    let mut runtime = PluggableRuntime::new(task_manager());
    runtime.set_source(source);
    runtime.set_package_loader(loader);

    let specifier: PackageSource = "acme/app".parse().unwrap();
    let pkg = BinaryPackage::from_registry(&specifier, &runtime)
        .await
        .unwrap();

    let command = pkg.get_command("after_deploy").unwrap();

    assert_eq!(command.package(), &root_id);
    assert_eq!(command.origin_package(), &dep_id);
    assert_eq!(
        pkg.get_command_origin_package("after_deploy"),
        Some(&dep_id)
    );
}
