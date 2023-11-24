use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Error;
use virtual_fs::TmpFileSystem;
use webc::metadata::annotations::Wasi as WasiAnnotation;

use crate::{
    bin_factory::BinaryPackage, capabilities::Capabilities, runners::MappedDirectory,
    WasiEnvBuilder,
};

#[derive(Debug, Clone)]
pub struct MappedCommand {
    /// The new alias.
    pub alias: String,
    /// The original command.
    pub target: String,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct CommonWasiOptions {
    pub(crate) args: Vec<String>,
    pub(crate) env: Vec<(String, String)>,
    pub(crate) forward_host_env: bool,
    pub(crate) mapped_dirs: Vec<MappedDirectory>,
    pub(crate) mapped_host_commands: Vec<MappedCommand>,
    pub(crate) injected_packages: Vec<BinaryPackage>,
    pub(crate) capabilities: Capabilities,
    pub(crate) fs: Option<TmpFileSystem>,
    pub(crate) current_dir: Option<PathBuf>,
}

impl CommonWasiOptions {
    pub(crate) fn prepare_webc_env(
        &self,
        builder: &mut WasiEnvBuilder,
        wasi: &WasiAnnotation,
        base_pkg: Option<&BinaryPackage>,
    ) -> Result<(), anyhow::Error> {
        for pkg in &self.injected_packages {
            builder.add_webc(pkg.clone());
        }

        let mapped_cmds = self
            .mapped_host_commands
            .iter()
            .map(|c| (c.alias.as_str(), c.target.as_str()));
        builder.add_mapped_commands(mapped_cmds);
        if let Some(pkg) = base_pkg {
            builder.set_package(pkg.clone());
        }

        self.populate_env(wasi, builder);
        self.populate_args(wasi, builder);

        *builder.capabilities_mut() = self.capabilities.clone();

        Ok(())
    }

    fn populate_env(&self, wasi: &WasiAnnotation, builder: &mut WasiEnvBuilder) {
        for item in wasi.env.as_deref().unwrap_or_default() {
            // TODO(Michael-F-Bryan): Convert "wasi.env" in the webc crate from an
            // Option<Vec<String>> to a HashMap<String, String> so we avoid this
            // string.split() business
            match item.split_once('=') {
                Some((k, v)) => {
                    builder.add_env(k, v);
                }
                None => {
                    builder.add_env(item, String::new());
                }
            }
        }

        if self.forward_host_env {
            builder.add_envs(std::env::vars());
        }

        builder.add_envs(self.env.clone());
    }

    fn populate_args(&self, wasi: &WasiAnnotation, builder: &mut WasiEnvBuilder) {
        if let Some(main_args) = &wasi.main_args {
            builder.add_args(main_args);
        }

        builder.add_args(&self.args);
    }

    pub(crate) fn set_filesystem(
        &self,
        builder: &mut WasiEnvBuilder,
        root_fs: TmpFileSystem,
    ) -> Result<(), Error> {
        builder.set_sandbox_fs(root_fs);
        builder.add_preopen_dir("/")?;
        // builder.add_preopen_dir("/home")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use tempfile::TempDir;

    use virtual_fs::WebcVolumeFileSystem;
    use webc::Container;

    use super::*;

    const PYTHON: &[u8] = include_bytes!("../../../c-api/examples/assets/python-0.1.0.wasmer");

    /// Fixes <https://github.com/wasmerio/wasmer/issues/3789>
    #[tokio::test]
    async fn mix_args_from_the_webc_and_user() {
        let args = CommonWasiOptions {
            args: vec!["extra".to_string(), "args".to_string()],
            ..Default::default()
        };
        let mut builder = WasiEnvBuilder::new("program-name");
        let fs = Arc::new(virtual_fs::EmptyFileSystem::default());
        let mut annotations = WasiAnnotation::new("some-atom");
        annotations.main_args = Some(vec![
            "hard".to_string(),
            "coded".to_string(),
            "args".to_string(),
        ]);

        args.prepare_webc_env(&mut builder, &annotations, None)
            .unwrap();

        assert_eq!(
            builder.get_args(),
            [
                // the program name from
                "program-name",
                // from the WEBC's annotations
                "hard",
                "coded",
                "args",
                // from the user
                "extra",
                "args",
            ]
        );
    }

    #[tokio::test]
    async fn mix_env_vars_from_the_webc_and_user() {
        let args = CommonWasiOptions {
            env: vec![("EXTRA".to_string(), "envs".to_string())]
                .into_iter()
                .collect(),
            ..Default::default()
        };
        let mut builder = WasiEnvBuilder::new("python");
        let fs = Arc::new(virtual_fs::EmptyFileSystem::default());
        let mut annotations = WasiAnnotation::new("python");
        annotations.env = Some(vec!["HARD_CODED=env-vars".to_string()]);

        args.prepare_webc_env(&mut builder, &annotations, None)
            .unwrap();

        assert_eq!(
            builder.get_env(),
            [
                ("HARD_CODED".to_string(), b"env-vars".to_vec()),
                ("EXTRA".to_string(), b"envs".to_vec()),
            ]
        );
    }

    // #[tokio::test]
    // async fn python_use_case() {
    //     let temp = TempDir::new().unwrap();
    //     let sub_dir = temp.path().join("path").join("to");
    //     std::fs::create_dir_all(&sub_dir).unwrap();
    //     std::fs::write(sub_dir.join("file.txt"), b"Hello, World!").unwrap();
    //     let container = Container::from_bytes(PYTHON).unwrap();
    //     let webc_fs = WebcVolumeFileSystem::mount_all(&container);
    //     let mut builder = WasiEnvBuilder::new("");

    //     let mut root_fs = RootFileSystemBuilder::default().build();
    //     let home = virtual_fs::host::FileSystem::new(sub_dir);
    //     root_fs.mount(PathBuf::from("/home"), home, PathBuf::new());
    //     let fs = prepare_filesystem(root_fs, Arc::new(webc_fs)).unwrap();

    //     assert!(fs.metadata("/home/file.txt".as_ref()).unwrap().is_file());
    //     assert!(fs.metadata("lib".as_ref()).unwrap().is_dir());
    //     assert!(fs
    //         .metadata("lib/python3.6/collections/__init__.py".as_ref())
    //         .unwrap()
    //         .is_file());
    //     assert!(fs
    //         .metadata("lib/python3.6/encodings/__init__.py".as_ref())
    //         .unwrap()
    //         .is_file());
    // }
}
