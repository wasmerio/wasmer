use std::{
    io::{self, *},
    sync::Arc,
};
use wasmer_vfs::FileDescriptor;
use wasmer_vfs::VirtualFile;

/// Special file for `/dev/tty` that can print to stdout
/// (hence the requirement for a `WasiRuntimeImplementation`)
#[derive(Debug)]
pub struct TtyFile {
    runtime: Arc<dyn crate::WasiRuntimeImplementation + Send + Sync + 'static>,
    stdin: Box<dyn VirtualFile + Send + Sync + 'static>,
}

impl TtyFile {
    pub fn new(
        runtime: Arc<dyn crate::WasiRuntimeImplementation + Send + Sync + 'static>,
        stdin: Box<dyn VirtualFile + Send + Sync + 'static>,
    ) -> Self {
        Self { runtime, stdin }
    }
}

impl Seek for TtyFile {
    fn seek(&mut self, _pos: SeekFrom) -> io::Result<u64> {
        Ok(0)
    }
}
impl Write for TtyFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.runtime.stdout(buf)?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Read for TtyFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.stdin.read(buf)
    }
}

impl VirtualFile for TtyFile {
    fn last_accessed(&self) -> u64 {
        self.stdin.last_accessed()
    }
    fn last_modified(&self) -> u64 {
        self.stdin.last_modified()
    }
    fn created_time(&self) -> u64 {
        self.stdin.created_time()
    }
    fn size(&self) -> u64 {
        0
    }
    fn set_len(&mut self, _new_size: u64) -> wasmer_vfs::Result<()> {
        Ok(())
    }
    fn unlink(&mut self) -> wasmer_vfs::Result<()> {
        Ok(())
    }
    fn bytes_available(&self) -> wasmer_vfs::Result<usize> {
        self.stdin.bytes_available()
    }
    fn bytes_available_read(&self) -> wasmer_vfs::Result<Option<usize>> {
        self.stdin.bytes_available_read()
    }
    fn bytes_available_write(&self) -> wasmer_vfs::Result<Option<usize>> {
        self.stdin.bytes_available_write()
    }
    fn get_fd(&self) -> Option<FileDescriptor> {
        None
    }
    fn is_open(&self) -> bool {
        true
    }
    fn get_special_fd(&self) -> Option<u32> {
        None
    }
}

#[cfg(test)]
mod tests {

    use crate::{VirtualNetworking, WasiRuntimeImplementation, WasiThreadId};
    use std::ops::Deref;
    use std::sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Mutex,
    };
    use wasmer_vbus::{UnsupportedVirtualBus, VirtualBus};

    struct FakeRuntimeImplementation {
        pub data: Arc<Mutex<Vec<u8>>>,
        pub bus: Box<dyn VirtualBus + Sync>,
        pub networking: Box<dyn VirtualNetworking + Sync>,
        pub thread_id_seed: AtomicU32,
    }

    impl Default for FakeRuntimeImplementation {
        fn default() -> Self {
            FakeRuntimeImplementation {
                data: Arc::new(Mutex::new(Vec::new())),
                #[cfg(not(feature = "host-vnet"))]
                networking: Box::new(wasmer_vnet::UnsupportedVirtualNetworking::default()),
                #[cfg(feature = "host-vnet")]
                networking: Box::new(wasmer_wasi_local_networking::LocalNetworking::default()),
                bus: Box::new(UnsupportedVirtualBus::default()),
                thread_id_seed: Default::default(),
            }
        }
    }

    impl FakeRuntimeImplementation {
        fn get_stdout_written(&self) -> Option<Vec<u8>> {
            let s = self.data.try_lock().ok()?;
            Some(s.clone())
        }
    }

    impl std::fmt::Debug for FakeRuntimeImplementation {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            write!(f, "FakeRuntimeImplementation")
        }
    }

    impl WasiRuntimeImplementation for FakeRuntimeImplementation {
        fn bus(&self) -> &(dyn VirtualBus) {
            self.bus.deref()
        }

        fn networking(&self) -> &(dyn VirtualNetworking) {
            self.networking.deref()
        }

        fn thread_generate_id(&self) -> WasiThreadId {
            self.thread_id_seed.fetch_add(1, Ordering::Relaxed).into()
        }

        fn stdout(&self, data: &[u8]) -> std::io::Result<()> {
            if let Ok(mut s) = self.data.try_lock() {
                s.extend_from_slice(data);
            }
            Ok(())
        }
    }

    #[test]
    fn test_tty_file() {
        use crate::state::WasiBidirectionalPipePair;
        use crate::tty_file::TtyFile;
        use std::io::Write;
        use std::sync::Arc;

        let mut pair = WasiBidirectionalPipePair::new();
        pair.set_blocking(false);

        let rt = Arc::new(FakeRuntimeImplementation::default());
        let mut tty_file = TtyFile::new(rt.clone(), Box::new(pair));
        tty_file.write(b"hello");
        assert_eq!(rt.get_stdout_written().unwrap(), b"hello".to_vec());
    }
}
