use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Error};
use virtual_fs::{FileSystem, OverlayFileSystem, RootFileSystemBuilder};
use webc::metadata::annotations::Wasi as WasiAnnotation;

use crate::{runners::MappedDirectory, WasiEnv, WasiEnvBuilder};

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct CommonWasiOptions {
    pub(crate) args: Vec<String>,
    pub(crate) env: HashMap<String, String>,
    pub(crate) forward_host_env: bool,
    pub(crate) mapped_dirs: Vec<MappedDirectory>,
}

impl CommonWasiOptions {
    pub(crate) fn prepare_webc_env(
        &self,
        container_fs: Arc<dyn FileSystem>,
        program_name: &str,
        wasi: &WasiAnnotation,
    ) -> Result<WasiEnvBuilder, anyhow::Error> {
        let mut builder = WasiEnv::builder(program_name).args(&self.args);

        let fs = prepare_filesystem(&self.mapped_dirs, container_fs, |path| {
            builder.add_preopen_dir(path).map_err(Error::from)
        })?;
        builder.set_fs(fs);
        builder.add_preopen_dir("/")?;
        builder.add_preopen_dir(".")?;

        self.populate_env(wasi, &mut builder);
        self.populate_args(wasi, &mut builder);

        Ok(builder)
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
}

fn prepare_filesystem(
    mapped_dirs: &[MappedDirectory],
    container_fs: Arc<dyn FileSystem>,
    mut preopen: impl FnMut(&Path) -> Result<(), Error>,
) -> Result<Box<dyn FileSystem + Send + Sync>, Error> {
    let root_fs = RootFileSystemBuilder::default().build();

    if !mapped_dirs.is_empty() {
        let host_fs: Arc<dyn FileSystem + Send + Sync> = Arc::new(crate::default_fs_backing());
        dbg!(mapped_dirs);

        for dir in mapped_dirs {
            let MappedDirectory { host, guest } = dir;
            let guest = PathBuf::from(guest);
            tracing::debug!(
                guest=%guest.display(),
                host=%host.display(),
                "Mounting host folder",
            );

            if let Some(parent) = guest.parent() {
                create_dir_all(&root_fs, parent).with_context(|| {
                    format!("Unable to create the \"{}\" directory", parent.display())
                })?;
            }

            root_fs
                .mount(guest.clone(), &host_fs, host.clone())
                .with_context(|| {
                    format!(
                        "Unable to mount \"{}\" to \"{}\"",
                        host.display(),
                        guest.display()
                    )
                })?;

            preopen(&guest)
                .with_context(|| format!("Unable to preopen \"{}\"", guest.display()))?;
        }
    }

    Ok(Box::new(OverlayFileSystem::new(root_fs, [container_fs])))
}

fn create_dir_all(fs: &dyn FileSystem, path: &Path) -> Result<(), Error> {
    if fs.metadata(path).is_ok() {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        create_dir_all(fs, parent)?;
    }

    fs.create_dir(path)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use crate::runners::WapmContainer;

    use super::*;

    const PYTHON: &[u8] = include_bytes!("../../../c-api/examples/assets/python-0.1.0.wasmer");

    #[test]
    fn python_use_case() {
        let temp = TempDir::new().unwrap();
        let sub_dir = temp.path().join("path").join("to");
        std::fs::create_dir_all(&sub_dir).unwrap();
        std::fs::write(sub_dir.join("file.txt"), b"Hello, World!").unwrap();
        let mapping = [MappedDirectory {
            guest: "/home".to_string(),
            host: sub_dir,
        }];
        let container = WapmContainer::from_bytes(PYTHON.into()).unwrap();

        let fs = prepare_filesystem(&mapping, container.container_fs(), |_| Ok(())).unwrap();

        assert!(fs.metadata("/home/file.txt".as_ref()).unwrap().is_file());
        assert!(fs.metadata("lib".as_ref()).unwrap().is_dir());
        assert!(fs
            .metadata("lib/python3.6/collections/__init__.py".as_ref())
            .unwrap()
            .is_file());
        assert!(fs
            .metadata("lib/python3.6/encodings/__init__.py".as_ref())
            .unwrap()
            .is_file());
    }
}
