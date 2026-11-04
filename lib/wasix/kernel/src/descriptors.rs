//! Generic descriptor custody. Rights and every other attachment policy belong to callers.

use crate::{
    Kernel,
    file::{BoundSlot, FdTable, OpenFile},
    sync::MutexExt,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
};
use wasixv2_syscalls::{
    Caller, Descriptor, DescriptorAttachment, DescriptorSet, DescriptorUpdate, Errno, Fd, TaskId,
};

struct BoundDescriptors {
    owner: Weak<()>,
    origin: TaskId,
    table: Weak<Mutex<FdTable>>,
    entries: HashMap<Fd, Option<BoundSlot>>,
}

/// Caller is engine-free; this provider-private payload supplies actual file identity.
pub(crate) fn capture(
    owner: &Arc<()>,
    origin: TaskId,
    table: &Arc<Mutex<FdTable>>,
    files: &FdTable,
    fds: &[Fd],
) -> DescriptorSet {
    let entries: HashMap<_, _> = fds.iter().map(|&fd| (fd, files.bind(fd))).collect();
    let descriptors = entries
        .iter()
        .map(|(&fd, slot)| Descriptor {
            fd,
            present: slot.is_some(),
            attachment: slot.as_ref().and_then(|slot| slot.attachment.clone()),
        })
        .collect();
    DescriptorSet::new(
        BoundDescriptors {
            owner: Arc::downgrade(owner),
            origin,
            table: Arc::downgrade(table),
            entries,
        },
        descriptors,
    )
}

impl Kernel {
    pub(crate) fn snapshot_fds(&self, task: TaskId, fds: &[Fd]) -> Result<DescriptorSet, Errno> {
        let state = self.state.read_or_recover();
        let table = &state.tasks.get(&task).ok_or(Errno::ESRCH)?.files;
        Ok(capture(
            &self.descriptor_owner,
            task,
            table,
            &table.lock_or_recover(),
            fds,
        ))
    }

    pub(crate) fn update_fd_attachment(
        &self,
        task: TaskId,
        set: &DescriptorSet,
        fd: Fd,
        replacement: Option<DescriptorAttachment>,
    ) -> Result<DescriptorUpdate, Errno> {
        let bound = set.payload::<BoundDescriptors>().ok_or(Errno::EBADF)?;
        if !Weak::ptr_eq(&bound.owner, &Arc::downgrade(&self.descriptor_owner)) {
            return Err(Errno::EBADF);
        }
        let Some(Some(expected)) = bound.entries.get(&fd) else {
            return Ok(DescriptorUpdate::Stale);
        };
        let state = self.state.read_or_recover();
        let Some(task) = state.tasks.get(&task) else {
            return Ok(DescriptorUpdate::Stale);
        };
        if !Weak::ptr_eq(&bound.table, &Arc::downgrade(&task.files)) {
            return Ok(DescriptorUpdate::Stale);
        }
        let mut table = task.files.lock_or_recover();
        let Some(current) = table
            .bind(fd)
            .filter(|slot| Arc::ptr_eq(&slot.token, &expected.token))
        else {
            return Ok(DescriptorUpdate::Stale);
        };
        let same = match (&current.attachment, &expected.attachment) {
            (None, None) => true,
            (Some(a), Some(b)) => a.ptr_eq(b),
            _ => false,
        };
        if !same {
            return Ok(DescriptorUpdate::Conflict);
        }
        table.replace_attachment(fd, replacement);
        Ok(DescriptorUpdate::Replaced)
    }

    /// The lock order is task table, then fd table. Holding the read guard through a slot
    /// mutation prevents an unshare/exec from moving the calling task to another table.
    pub(crate) fn with_descriptor_table<T>(
        &self,
        caller: &Caller<'_>,
        f: impl FnOnce(&Arc<Mutex<FdTable>>, &mut FdTable) -> Result<T, Errno>,
    ) -> Result<T, Errno> {
        let state = self.state.read_or_recover();
        let table = &state.tasks.get(&caller.task()).ok_or(Errno::ESRCH)?.files;
        if let Some(set) = caller.descriptors() {
            let bound = set.payload::<BoundDescriptors>().ok_or(Errno::EBADF)?;
            if !Weak::ptr_eq(&bound.owner, &Arc::downgrade(&self.descriptor_owner)) {
                return Err(Errno::EBADF);
            }
            if !Weak::ptr_eq(&bound.table, &Arc::downgrade(table)) {
                return Err(Errno::EBADF);
            }
        }
        f(table, &mut table.lock_or_recover())
    }

    pub(crate) fn bound_slot(
        &self,
        caller: &Caller<'_>,
        files: &FdTable,
        fd: Fd,
    ) -> Result<BoundSlot, Errno> {
        match caller.descriptors() {
            None => files.bind(fd).ok_or(Errno::EBADF),
            Some(set) => {
                let slot = set
                    .payload::<BoundDescriptors>()
                    .and_then(|bound| bound.entries.get(&fd))
                    .and_then(Option::as_ref)
                    .ok_or(Errno::EBADF)?;
                if !files.matches(fd, &slot.token) {
                    return Err(Errno::EBADF);
                }
                Ok(slot.clone())
            }
        }
    }

    pub(crate) fn caller_file(&self, caller: &Caller<'_>, fd: Fd) -> Result<Arc<OpenFile>, Errno> {
        let Some(set) = caller.descriptors() else {
            return self.file(caller.task(), fd);
        };
        self.admitted_descriptors(caller, set)?
            .entries
            .get(&fd)
            .and_then(Option::as_ref)
            .map(|slot| slot.file.clone())
            .ok_or(Errno::EBADF)
    }

    pub(crate) fn poll_file(
        &self,
        caller: &Caller<'_>,
        table: &Mutex<FdTable>,
        fd: Fd,
    ) -> Result<Option<Arc<OpenFile>>, Errno> {
        let Some(set) = caller.descriptors() else {
            return Ok(table.lock_or_recover().get(fd));
        };
        let slot = self
            .admitted_descriptors(caller, set)?
            .entries
            .get(&fd)
            .ok_or(Errno::EBADF)?;
        Ok(slot.as_ref().map(|slot| slot.file.clone()))
    }

    pub(crate) fn descriptor_cloexec(&self, caller: &Caller<'_>, fd: Fd) -> Result<bool, Errno> {
        if let Some(set) = caller.descriptors() {
            return self
                .admitted_descriptors(caller, set)?
                .entries
                .get(&fd)
                .and_then(Option::as_ref)
                .map(|slot| slot.cloexec)
                .ok_or(Errno::EBADF);
        }
        self.with_descriptor_table(caller, |_, files| {
            Ok(self.bound_slot(caller, files, fd)?.cloexec)
        })
    }

    fn admitted_descriptors<'a>(
        &self,
        caller: &Caller<'_>,
        set: &'a DescriptorSet,
    ) -> Result<&'a BoundDescriptors, Errno> {
        let bound = set.payload::<BoundDescriptors>().ok_or(Errno::EBADF)?;
        if !Weak::ptr_eq(&bound.owner, &Arc::downgrade(&self.descriptor_owner)) {
            return Err(Errno::EBADF);
        }
        // Contexts share a TaskId. An already admitted operation retains its exact objects
        // even when another context unshares the table before the operation resumes.
        if bound.origin != caller.task() {
            let state = self.state.read_or_recover();
            let task = state.tasks.get(&caller.task()).ok_or(Errno::ESRCH)?;
            if !Weak::ptr_eq(&bound.table, &Arc::downgrade(&task.files)) {
                return Err(Errno::EBADF);
            }
        }
        Ok(bound)
    }
}
