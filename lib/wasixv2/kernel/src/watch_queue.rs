//! Linux kernel/watch_queue.c: whole-record notification pipes and object watch lists.

use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use wasixv2_abi::{GuestLayout, GuestPtr, WatchNotificationFilter, WatchNotificationTypeFilter};
use wasixv2_syscalls::{Caller, Errno, Fd, read_value};

use crate::{Kernel, pipe::Pipe, sync::MutexExt};

const NOTE_SIZE: usize = 128;
const NOTES_PER_PAGE: usize = 4096 / NOTE_SIZE;
const INFO_ID: u32 = 0xff00;

#[derive(Default)]
pub(crate) struct Users(Mutex<HashMap<u32, Arc<AtomicUsize>>>);

impl Users {
    fn user(&self, uid: u32) -> Arc<AtomicUsize> {
        self.0.lock_or_recover().entry(uid).or_default().clone()
    }
}

struct Note {
    bytes: Vec<u8>,
    loss: bool,
}

#[derive(Default)]
pub(crate) struct Queue {
    limit: Option<usize>,
    filter: Option<Vec<WatchNotificationTypeFilter>>,
    notes: VecDeque<Note>,
    note_loss: bool,
    pub(crate) watches: Vec<Weak<Watch>>,
}

impl Queue {
    /// watch_queue_set_size takes a number of notifications, despite the UAPI comment.
    pub(crate) fn set_size(&mut self, notes: u32) -> Result<usize, Errno> {
        if self.limit.is_some() {
            return Err(Errno::EBUSY);
        }
        if !(1..=512).contains(&notes) {
            return Err(Errno::EINVAL);
        }
        let pages = (notes as usize).div_ceil(NOTES_PER_PAGE);
        self.limit = Some(pages * NOTES_PER_PAGE);
        Ok(pages * 4096)
    }

    pub(crate) fn set_filter(&mut self, filter: Option<Vec<WatchNotificationTypeFilter>>) {
        self.filter = filter;
    }

    fn accepts(&self, bytes: &[u8]) -> bool {
        let packed = u32::from_le_bytes(bytes[..4].try_into().unwrap());
        let (kind, subtype) = (packed & 0xff_ffff, packed >> 24);
        let info = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        self.filter.as_ref().is_none_or(|filters| {
            filters.iter().any(|f| {
                f.r#type == kind
                    && subtype < 32
                    && f.subtype_filter[0] & (1 << subtype) != 0
                    && info & f.info_mask == f.info_filter
            })
        })
    }

    /// post_one_notification sets LOSS on the last queued buffer; the loss record follows it.
    pub(crate) fn post(&mut self, bytes: Vec<u8>, filtered: bool) -> bool {
        if filtered && !self.accepts(&bytes) {
            return false;
        }
        let limit = self.limit.unwrap_or(0);
        if self.notes.len() >= limit {
            if let Some(last) = self.notes.back_mut() {
                last.loss = true;
            }
            return false;
        }
        self.notes.push_back(Note { bytes, loss: false });
        true
    }

    pub(crate) fn readable(&self) -> bool {
        self.note_loss || !self.notes.is_empty()
    }

    pub(crate) fn len(&self) -> usize {
        self.notes.iter().map(|n| n.bytes.len()).sum()
    }

    pub(crate) fn front(&self) -> Option<Vec<u8>> {
        if self.note_loss {
            // WATCH_TYPE_META, WATCH_META_LOSS_NOTIFICATION, sizeof(watch_notification).
            return Some([0x0100_0000u32.to_le_bytes(), 8u32.to_le_bytes()].concat());
        }
        self.notes.front().map(|n| n.bytes.clone())
    }

    pub(crate) fn pop(&mut self) {
        if self.note_loss {
            self.note_loss = false;
        } else if let Some(note) = self.notes.pop_front() {
            self.note_loss = note.loss;
        }
    }
}

impl Drop for Queue {
    fn drop(&mut self) {
        // watch_queue_clear: closing both pipe ends immediately returns user watch charges.
        for watch in self.watches.iter().filter_map(Weak::upgrade) {
            watch.release();
        }
    }
}

/// Decodes the variable-length Linux filter before replacing the old one atomically.
pub(crate) async fn read_filter(
    caller: &Caller<'_>,
    addr: u32,
) -> Result<Option<Vec<WatchNotificationTypeFilter>>, wasixv2_syscalls::SysErr> {
    if addr == 0 {
        return Ok(None);
    }
    let header: WatchNotificationFilter = read_value(caller.memory(), GuestPtr::new(addr)).await?;
    if !(1..=16).contains(&header.nr_filters) || header.reserved != 0 {
        return Err(Errno::EINVAL.into());
    }
    let mut filters = Vec::new();
    for i in 0..header.nr_filters {
        let offset = addr
            .checked_add(8 + i * WatchNotificationTypeFilter::SIZE as u32)
            .ok_or(Errno::EFAULT)?;
        let filter: WatchNotificationTypeFilter =
            read_value(caller.memory(), GuestPtr::new(offset)).await?;
        if filter.info_filter & !filter.info_mask != 0 || filter.info_mask & 0x7f != 0 {
            return Err(Errno::EINVAL.into());
        }
        // kernel/watch_queue.c watch_queue_set_filter ignores unknown types.
        if filter.r#type < 2 {
            filters.push(filter);
        }
    }
    Ok(Some(filters))
}

pub(crate) struct Watch {
    queue: Weak<Pipe>,
    object: u64,
    info: u32,
    user: Arc<AtomicUsize>,
    charged: AtomicBool,
}

impl Watch {
    fn release(&self) {
        if self.charged.swap(false, Ordering::AcqRel) {
            self.user.fetch_sub(1, Ordering::AcqRel);
        }
    }

    fn removal(&self) {
        let len = if self.object == 0 { 8 } else { 16 };
        let mut bytes = [0u32.to_le_bytes(), (len | self.info).to_le_bytes()].concat();
        if self.object != 0 {
            bytes.extend_from_slice(&self.object.to_le_bytes());
        }
        if let Some(queue) = self.queue.upgrade() {
            queue.post_notification(bytes, false);
        }
        self.release();
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.release();
    }
}

/// A kernel-side notification source, analogous to Linux's watch_list. Each object owns a
/// list; keyring integration checks KEY_NEED_VIEW before registering on it. Lists do not keep
/// notification pipes or the watched object alive.
#[derive(Default)]
pub struct WatchList(Mutex<Vec<Arc<Watch>>>);

impl WatchList {
    /// Adds a watch, or removes it for watch_id -1. The source's owner must check object
    /// permissions first. queue_fd may name either end of an O_NOTIFICATION_PIPE.
    pub fn watch(
        &self,
        kernel: &Kernel,
        caller: &Caller<'_>,
        object: u64,
        queue_fd: Fd,
        watch_id: i32,
    ) -> Result<(), Errno> {
        if !(-1..=255).contains(&watch_id) {
            return Err(Errno::EINVAL);
        }
        // get_watch_queue returns EINVAL even for an invalid descriptor.
        let file = kernel
            .io_file(caller, queue_fd)
            .map_err(|_| Errno::EINVAL)?;
        let queue = crate::pipe::pipe_info(&file)
            .filter(|p| p.is_notification())
            .ok_or(Errno::EINVAL)?;
        let mut list = self.0.lock_or_recover();
        list.retain(|w| w.charged.load(Ordering::Acquire));
        let found = list
            .iter()
            .position(|w| w.object == object && w.queue.ptr_eq(&Arc::downgrade(&queue)));
        if watch_id == -1 {
            let index = found.ok_or(Errno::EBADSLT)?;
            list.remove(index).removal();
            return Ok(());
        }
        if found.is_some() {
            return Err(Errno::EBUSY);
        }
        let user = kernel.watch_users.user(kernel.cred(caller).user.real.0);
        let max = kernel
            .process(caller.task())
            .resources
            .lock_or_recover()
            .nofile();
        user.fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
            (n < max).then_some(n + 1)
        })
        .map_err(|_| Errno::EAGAIN)?;
        let watch = Arc::new(Watch {
            queue: Arc::downgrade(&queue),
            object,
            info: (watch_id as u32) << 8,
            user,
            charged: AtomicBool::new(true),
        });
        queue.add_notification_watch(Arc::downgrade(&watch));
        list.push(watch);
        Ok(())
    }

    /// Posts a Linux watch_notification and its payload. The record must contain its byte
    /// length in info's low seven bits and have a length of 8..=127; the source checks its
    /// own notification type/layout. Each watch supplies its own info ID.
    pub fn post(&self, object: u64, notification: &[u8]) {
        if !(8..=127).contains(&notification.len())
            || notification[4] & 0x7f != notification.len() as u8
        {
            return;
        }
        let list = self.0.lock_or_recover();
        for watch in list.iter().filter(|w| w.object == object) {
            if let Some(queue) = watch.queue.upgrade() {
                let mut bytes = notification.to_vec();
                let info = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
                bytes[4..8].copy_from_slice(&(info & !INFO_ID | watch.info).to_le_bytes());
                queue.post_notification(bytes, true);
            }
        }
    }
}

impl Drop for WatchList {
    fn drop(&mut self) {
        for watch in self.0.get_mut().unwrap_or_else(|e| e.into_inner()) {
            watch.removal();
        }
    }
}
