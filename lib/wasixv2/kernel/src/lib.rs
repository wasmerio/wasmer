//! The kernel: tasks, processes, credentials, signals, namespaces, IPC, memory, terminals, pseudo
//! filesystems and the socket layer. Implements `Syscalls`.
//!
//! Knows nothing about the wasm engine, the layers, or concrete filesystem drivers and network
//! backends.

mod file;
mod stdio;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

pub use file::FileObject;
pub use stdio::Stdio;
use wasixv2_syscalls::{
    Caller, Control, Errno, Fd, GuestAction, SysResult, Syscalls, TaskId, UserSlice, UserSliceMut,
};

use crate::file::{FdTable, OpenFile};

/// Most bytes one `read` or `write` moves. Partial transfers are valid for both, and this bounds
/// the host allocation a guest-chosen length can cause.
const IO_CHUNK: usize = 64 * 1024;

#[derive(Default)]
pub struct Kernel {
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    last_id: u32,
    tasks: HashMap<TaskId, Task>,
}

struct Task {
    files: Arc<Mutex<FdTable>>,
    pending: Option<GuestAction>,
}

impl Kernel {
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts a process with a single task whose descriptors 0, 1 and 2 are `stdio`.
    pub fn spawn(&self, stdio: Stdio) -> TaskId {
        let mut files = FdTable::default();
        for object in stdio.into_objects() {
            files.install(Arc::new(OpenFile { object }));
        }
        let mut state = self.state.lock().unwrap();
        state.last_id += 1;
        let id = TaskId(state.last_id);
        let task = Task {
            files: Arc::new(Mutex::new(files)),
            pending: None,
        };
        state.tasks.insert(id, task);
        id
    }

    fn file(&self, task: TaskId, fd: Fd) -> Result<Arc<OpenFile>, Errno> {
        let files = self.with_task(task, |task| task.files.clone());
        let file = files.lock().unwrap().get(fd);
        file.ok_or(Errno::EBADF)
    }

    fn with_task<R>(&self, task: TaskId, f: impl FnOnce(&mut Task) -> R) -> R {
        let mut state = self.state.lock().unwrap();
        let task = state
            .tasks
            .get_mut(&task)
            .expect("syscall from an unknown task");
        f(task)
    }
}

impl Syscalls for Kernel {
    async fn read(&self, caller: &Caller<'_>, fd: Fd, buf: UserSliceMut) -> SysResult<u32> {
        let file = self.file(caller.task(), fd)?;
        let mut data = vec![0; (buf.len as usize).min(IO_CHUNK)];
        let n = file.object.read(&mut data).await?;
        buf.copy_out(caller.memory(), &data[..n]).await?;
        Ok(n as u32)
    }

    async fn write(&self, caller: &Caller<'_>, fd: Fd, buf: UserSlice) -> SysResult<u32> {
        let file = self.file(caller.task(), fd)?;
        let data = buf.copy_in(caller.memory(), IO_CHUNK).await?;
        let n = file.object.write(&data).await?;
        Ok(n as u32)
    }

    async fn exit_group(&self, caller: &Caller<'_>, status: i32) -> SysResult<()> {
        // Linux keeps only the low byte of the status.
        let action = GuestAction::Exit(status & 0xff);
        self.with_task(caller.task(), |task| task.pending = Some(action));
        Ok(())
    }
}

impl Control for Kernel {
    fn take_pending(&self, task: TaskId) -> Option<GuestAction> {
        self.with_task(task, |task| task.pending.take())
    }
}

#[cfg(test)]
mod tests;
