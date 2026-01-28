use std::{path::Path, sync::Arc};

use futures::future::BoxFuture;
use virtual_fs::{FileSystem, FsError, OpenOptions, OpenOptionsConfig};

#[derive(Debug)]
pub struct RelativeOrAbsolutePathHack<F>(pub F);

impl<F: FileSystem> RelativeOrAbsolutePathHack<F> {
    fn execute<Func, Ret>(&self, path: &Path, operation: Func) -> Result<Ret, FsError>
    where
        Func: Fn(&F, &Path) -> Result<Ret, FsError>,
    {
        // First, try it with the path we were given
        let result = operation(&self.0, path);

        if result.is_err() && !path.is_absolute() {
            // we were given a relative path, but maybe the operation will work
            // using absolute paths instead.
            let path = Path::new("/").join(path);
            operation(&self.0, &path)
        } else {
            result
        }
    }
}

impl<F: FileSystem> virtual_fs::FileSystem for RelativeOrAbsolutePathHack<F> {
    fn readlink(&self, path: &Path) -> virtual_fs::Result<std::path::PathBuf> {
        self.execute(path, |fs, p| fs.readlink(p))
    }

    fn read_dir(&self, path: &Path) -> virtual_fs::Result<virtual_fs::ReadDir> {
        self.execute(path, |fs, p| fs.read_dir(p))
    }

    fn create_dir(&self, path: &Path) -> virtual_fs::Result<()> {
        self.execute(path, |fs, p| fs.create_dir(p))
    }

    fn remove_dir(&self, path: &Path) -> virtual_fs::Result<()> {
        self.execute(path, |fs, p| fs.remove_dir(p))
    }

    fn rename<'a>(&'a self, from: &Path, to: &Path) -> BoxFuture<'a, virtual_fs::Result<()>> {
        let from = from.to_owned();
        let to = to.to_owned();
        Box::pin(async move { self.0.rename(&from, &to).await })
    }

    fn metadata(&self, path: &Path) -> virtual_fs::Result<virtual_fs::Metadata> {
        self.execute(path, |fs, p| fs.metadata(p))
    }

    fn symlink_metadata(&self, path: &Path) -> virtual_fs::Result<virtual_fs::Metadata> {
        self.execute(path, |fs, p| fs.symlink_metadata(p))
    }

    fn unlink(&self, path: &Path) -> virtual_fs::Result<()> {
        self.execute(path, |fs, p| fs.unlink(p))
    }

    fn new_open_options(&self) -> OpenOptions<'_> {
        virtual_fs::OpenOptions::new(self)
    }

    fn mount(
        &self,
        name: String,
        path: &Path,
        fs: Box<dyn FileSystem + Send + Sync>,
    ) -> virtual_fs::Result<()> {
        let name_ref = &name;
        let f_ref = &Arc::new(fs);
        self.execute(path, move |f, p| {
            f.mount(name_ref.clone(), p, Box::new(f_ref.clone()))
        })
    }
}

impl<F: FileSystem> virtual_fs::FileOpener for RelativeOrAbsolutePathHack<F> {
    fn open(
        &self,
        path: &Path,
        conf: &OpenOptionsConfig,
    ) -> virtual_fs::Result<Box<dyn virtual_fs::VirtualFile + Send + Sync + 'static>> {
        self.execute(path, |fs, p| {
            fs.new_open_options().options(conf.clone()).open(p)
        })
    }
}
