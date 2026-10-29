//! Filesystem operations, following fs/fuse/{inode,dir,file,xattr,readdir}.c.
use super::{Connection, wire::*};
use crate::{
    sync::MutexExt,
    uidgid::{Kgid, Kuid},
};
use async_trait::async_trait;
use std::{
    any::Any,
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use wasixv2_syscalls::Errno;
use wasixv2_vfs::{
    AclKind, AllocMode, Attr, Cred, Device, DirEntry, DirSink, FileType, Filesystem, FsInfo,
    FsResult, Handle, NewKind, NewNode, Node, OpenOptions, PosixAcl, RenameMode, SeekHole, SetAttr,
    StatFs, TimeSet, Timestamp, XattrSet,
};

pub(super) struct FuseFs {
    root: Arc<FuseNode>,
    conn: Arc<Connection>,
}
impl FuseFs {
    pub(super) fn new(conn: Arc<Connection>) -> Self {
        let mode = conn.options.mode.unwrap();
        let root = Arc::new(FuseNode {
            id: 1,
            generation: 0,
            kind: FileType::from_mode(mode).unwrap(),
            conn: conn.clone(),
            attr: Mutex::new((
                Attr {
                    ino: 1,
                    file_type: FileType::from_mode(mode).unwrap(),
                    mode: mode & 0o7777,
                    nlink: 1,
                    uid: conn.options.uid,
                    gid: conn.options.gid,
                    rdev: Device::default(),
                    size: 0,
                    blocks: 0,
                    blksize: 4096,
                    atime: Timestamp::default(),
                    mtime: Timestamp::default(),
                    ctime: Timestamp::default(),
                    btime: None,
                    dev: None,
                },
                Duration::ZERO,
            )),
            entries: Mutex::new(BTreeMap::new()),
            nlookup: AtomicU64::new(0),
            acl_epoch: AtomicU64::new(0),
            deleted: AtomicBool::new(false),
            data_lock: futures_util::lock::Mutex::new(()),
        });
        conn.nodes
            .lock_or_recover()
            .insert(1, Arc::downgrade(&root));
        Self { root, conn }
    }
}
impl Drop for FuseFs {
    fn drop(&mut self) {
        // fuse_put_super/fuse_send_destroy: queued after all open files stop holding the sb.
        // No user code runs from Drop; the daemon completes this background request.
        self.conn.destroy();
    }
}
#[async_trait(?Send)]
impl Filesystem for FuseFs {
    fn limits_file_size(&self) -> bool {
        true
    }

    fn info(&self) -> FsInfo {
        FsInfo {
            name: "fuse",
            magic: 0x65735546,
            name_max: 255,
            read_only: false,
            dev: None,
            has_btime: false,
            stable: false,
        }
    }
    fn subtype(&self) -> Option<&[u8]> {
        self.conn.options.subtype.as_deref()
    }
    fn umount_begin(&self) {
        self.conn.abort();
    }
    async fn shutdown(&self) {
        if let Some(req) = self.conn.destroy() {
            let _ = wasixv2_wait::wait_event(
                &req.wait,
                || req.reply.lock_or_recover().take(),
                &wasixv2_wait::NoInterrupt,
            )
            .await;
        }
    }
    async fn sync(&self) -> FsResult<()> {
        if self.conn.queue.lock_or_recover().minor < 36 {
            return Ok(());
        }
        match self.conn.optional(50, 1, vec![0; 8], 0, 0).await {
            Ok(_) | Err(Errno::ENOSYS) => Ok(()),
            Err(e) => Err(e),
        }
    }
    fn root(&self) -> Arc<dyn Node> {
        self.root.clone()
    }
    async fn initialize(&self) -> FsResult<()> {
        if self.conn.sync_init {
            self.conn.ready().await
        } else {
            Ok(())
        }
    }
    async fn statfs(&self) -> FsResult<StatFs> {
        if self.conn.allowed().is_err() {
            return Ok(StatFs::default());
        }
        let out = self.conn.request(17, 1, Vec::new(), 80, 80).await?;
        Ok(StatFs {
            blocks: u64_at(&out, 0)?,
            blocks_free: u64_at(&out, 8)?,
            blocks_avail: u64_at(&out, 16)?,
            files: u64_at(&out, 24)?,
            files_free: u64_at(&out, 32)?,
            block_size: u32_at(&out, 40)?,
            name_max: u32_at(&out, 44)?,
            fragment_size: u32_at(&out, 48)?,
        })
    }
    fn show_options(&self, out: &mut Vec<u8>) {
        let o = &self.conn.options;
        out.extend_from_slice(
            format!(
                ",user_id={},group_id={}",
                self.conn.user_ns.munged_uid(Kuid(o.uid)),
                self.conn.user_ns.munged_gid(Kgid(o.gid))
            )
            .as_bytes(),
        );
        if self.conn.default_permissions() {
            out.extend_from_slice(b",default_permissions");
        }
        if o.allow_other {
            out.extend_from_slice(b",allow_other");
        }
        if let Some(max) = o.max_read {
            out.extend_from_slice(format!(",max_read={max}").as_bytes());
        }
    }
}

struct CachedEntry {
    node: Option<Weak<FuseNode>>,
    until: Duration,
}
pub(super) struct FuseNode {
    id: u64,
    generation: u64,
    kind: FileType,
    conn: Arc<Connection>,
    attr: Mutex<(Attr, Duration)>,
    entries: Mutex<BTreeMap<Vec<u8>, CachedEntry>>,
    nlookup: AtomicU64,
    acl_epoch: AtomicU64,
    deleted: AtomicBool,
    data_lock: futures_util::lock::Mutex<()>,
}
impl Drop for FuseNode {
    fn drop(&mut self) {
        let count = self.nlookup.load(Ordering::Relaxed);
        if count != 0 {
            self.conn.forget(self.id, count);
        }
    }
}
impl FuseNode {
    pub(super) fn invalidate(&self) {
        self.attr.lock_or_recover().1 = Duration::ZERO;
        self.acl_epoch.fetch_add(1, Ordering::Release);
    }
    pub(super) fn invalidate_entry(&self, name: &[u8]) {
        self.entries.lock_or_recover().remove(name);
        self.invalidate();
    }
    pub(super) fn notify_entry(&self, name: &[u8], child: u64) -> FsResult<()> {
        if self.kind != FileType::Directory {
            return Err(Errno::ENOTDIR);
        }
        let entry = self
            .entries
            .lock_or_recover()
            .remove(name)
            .ok_or(Errno::ENOENT)?;
        self.invalidate();
        if child != 0
            && let Some(node) = entry.node.and_then(|node| node.upgrade())
        {
            if node.id != child {
                return Err(Errno::ENOENT);
            }
            if node.kind == FileType::Directory {
                node.deleted.store(true, Ordering::Release);
            }
            node.attr.lock_or_recover().0.nlink = 0;
        }
        Ok(())
    }
    fn decode_attr(conn: &Connection, out: &[u8]) -> FsResult<Attr> {
        if out.len() < 88 {
            return Err(Errno::EIO);
        }
        if u64_at(out, 8)? > i64::MAX as u64 {
            return Err(Errno::EIO);
        }
        let mode = u32_at(out, 60)?;
        let kind = FileType::from_mode(mode).ok_or(Errno::EIO)?;
        let time = |at, ns| -> FsResult<Timestamp> {
            let nsec = u32_at(out, ns)?;
            if nsec >= 1_000_000_000 {
                return Err(Errno::EIO);
            }
            Ok(Timestamp::new(u64_at(out, at)? as i64, nsec))
        };
        Ok(Attr {
            ino: u64_at(out, 0)?,
            file_type: kind,
            mode: mode & 0o7777,
            size: u64_at(out, 8)?,
            blocks: u64_at(out, 16)?,
            atime: time(24, 48)?,
            mtime: time(32, 52)?,
            ctime: time(40, 56)?,
            nlink: u32_at(out, 64)?,
            uid: conn
                .user_ns
                .make_kuid(u32_at(out, 68)?)
                .map_or(u32::MAX, |id| id.0),
            gid: conn
                .user_ns
                .make_kgid(u32_at(out, 72)?)
                .map_or(u32::MAX, |id| id.0),
            rdev: Device::from_dev_t(u32_at(out, 76)?),
            blksize: u32_at(out, 80)?,
            btime: None,
            dev: None,
        })
    }
    fn update_attr(&self, bytes: &[u8], secs: u64, nanos: u32) -> FsResult<Attr> {
        let attr = Self::decode_attr(&self.conn, bytes)?;
        if attr.file_type != self.kind {
            return Err(Errno::EIO);
        }
        *self.attr.lock_or_recover() = (attr.clone(), self.conn.deadline(secs, nanos));
        self.acl_epoch.fetch_add(1, Ordering::Release);
        Ok(attr)
    }
    /// fuse_iget: wire nodeid is identity, attr.ino is the stat number the daemon supplies.
    fn entry(&self, name: &[u8], out: &[u8]) -> FsResult<Arc<FuseNode>> {
        let result = self.link_entry(name, out);
        if result.is_err()
            && let Ok(id) = u64_at(out, 0)
            && id > 1
        {
            self.conn.forget(id, 1);
        }
        result
    }
    fn link_entry(&self, name: &[u8], out: &[u8]) -> FsResult<Arc<FuseNode>> {
        if out.len() < 128 {
            return Err(Errno::EIO);
        }
        let id = u64_at(out, 0)?;
        let until = self.conn.deadline(u64_at(out, 16)?, u32_at(out, 32)?);
        if id == 0 {
            self.entries
                .lock_or_recover()
                .insert(name.to_vec(), CachedEntry { node: None, until });
            return Err(Errno::ENOENT);
        }
        if id == 1 {
            return Err(Errno::EIO);
        }
        let attr = Self::decode_attr(&self.conn, &out[40..])?;
        let generation = u64_at(out, 8)?;
        let attr_until = self.conn.deadline(u64_at(out, 24)?, u32_at(out, 36)?);
        let node = {
            let mut nodes = self.conn.nodes.lock_or_recover();
            if let Some(node) = nodes.get(&id).and_then(Weak::upgrade) {
                if node.kind != attr.file_type || node.generation != generation {
                    return Err(Errno::EIO);
                }
                *node.attr.lock_or_recover() = (attr, attr_until);
                node.acl_epoch.fetch_add(1, Ordering::Release);
                node.nlookup.fetch_add(1, Ordering::Relaxed);
                node
            } else {
                let node = Arc::new(FuseNode {
                    id,
                    generation,
                    kind: attr.file_type,
                    conn: self.conn.clone(),
                    attr: Mutex::new((attr, attr_until)),
                    entries: Mutex::new(BTreeMap::new()),
                    nlookup: AtomicU64::new(1),
                    acl_epoch: AtomicU64::new(0),
                    deleted: AtomicBool::new(false),
                    data_lock: futures_util::lock::Mutex::new(()),
                });
                nodes.insert(id, Arc::downgrade(&node));
                node
            }
        };
        self.entries.lock_or_recover().insert(
            name.to_vec(),
            CachedEntry {
                node: Some(Arc::downgrade(&node)),
                until,
            },
        );
        Ok(node)
    }
    async fn refresh(&self) -> FsResult<Attr> {
        let out = self.conn.request(3, self.id, vec![0; 16], 104, 104).await?;
        self.update_attr(&out[16..], u64_at(&out, 0)?, u32_at(&out, 8)?)
    }
    async fn set(&self, set: &SetAttr, fh: Option<u64>) -> FsResult<Attr> {
        let mut body = vec![0; 88];
        let mut valid = 0;
        if let Some(fh) = fh {
            valid |= 1 << 6;
            put64(&mut body, 8, fh);
        }
        if let Some(mode) = set.mode {
            valid |= 1;
            put32(&mut body, 68, self.kind.mode_bits() | mode);
        }
        if let Some(uid) = set.uid {
            valid |= 2;
            put32(&mut body, 76, self.conn.user_ns.munged_uid(Kuid(uid)));
        }
        if let Some(gid) = set.gid {
            valid |= 4;
            put32(&mut body, 80, self.conn.user_ns.munged_gid(Kgid(gid)));
        }
        if let Some(size) = set.size {
            valid |= 8;
            put64(&mut body, 16, size);
        }
        for (value, bit, now_bit, at, ns) in [
            (set.atime, 4, 7, 32, 56),
            (set.mtime, 5, 8, 40, 60),
            (set.ctime, 10, 10, 48, 64),
        ] {
            if let Some(value) = value {
                valid |= 1 << bit;
                let stamp = match value {
                    TimeSet::At(t) => t,
                    TimeSet::Now => {
                        valid |= 1 << now_bit;
                        let now = self.conn.host.realtime();
                        Timestamp::new(now.as_secs() as i64, now.subsec_nanos())
                    }
                };
                put64(&mut body, at, stamp.sec as u64);
                put32(&mut body, ns, stamp.nsec);
            }
        }
        put32(&mut body, 0, valid);
        let out = self.conn.request(4, self.id, body, 104, 104).await?;
        self.update_attr(&out[16..], u64_at(&out, 0)?, u32_at(&out, 8)?)
    }
    fn flags(options: OpenOptions) -> u32 {
        (if options.write {
            if options.read { 2 } else { 1 }
        } else {
            0
        }) | 0o100000
            | if options.sync { 0o4010000 } else { 0 }
    }
    async fn open_handle(&self, options: OpenOptions) -> FsResult<FuseHandle> {
        let dir = self.kind == FileType::Directory;
        let opcode = if dir { 27 } else { 14 };
        let flags = Self::flags(options);
        let out = self
            .conn
            .optional(opcode, self.id, words32(&[flags, 0]), 16, 16)
            .await;
        let (fh, open_flags, no_open) = match out {
            Ok(out) => (u64_at(&out, 0)?, u32_at(&out, 8)?, false),
            Err(Errno::ENOSYS) => (0, 0, true),
            Err(e) => return Err(e),
        };
        Ok(FuseHandle {
            node: self.arc()?,
            fh,
            flags,
            open_flags,
            dir,
            no_open,
            poll: self.new_poll(),
            cookies: Mutex::new(DirCookies::default()),
        })
    }
    fn new_poll(&self) -> Arc<super::poll::PollState> {
        let key = self.conn.next_poll.fetch_add(1, Ordering::Relaxed);
        let poll = super::poll::PollState::new(key);
        self.conn
            .polls
            .lock_or_recover()
            .insert(key, Arc::downgrade(&poll));
        poll
    }
    fn arc(&self) -> FsResult<Arc<Self>> {
        self.conn
            .nodes
            .lock_or_recover()
            .get(&self.id)
            .and_then(Weak::upgrade)
            .ok_or(Errno::ESTALE)
    }
    async fn remove(&self, name: &[u8], opcode: u32) -> FsResult<()> {
        let mut body = Vec::new();
        super::wire::name(&mut body, name);
        self.conn.request(opcode, self.id, body, 0, 0).await?;
        self.invalidate_entry(name);
        Ok(())
    }
}
#[async_trait(?Send)]
impl Node for FuseNode {
    fn unlinked_directory(&self) -> bool {
        self.deleted.load(Ordering::Acquire)
    }
    fn acl_permissions(&self) -> bool {
        self.conn.queue.lock_or_recover().flags & (1 << 20) != 0
    }
    fn acl_version(&self) -> u64 {
        self.acl_epoch.load(Ordering::Acquire)
    }
    fn ino(&self) -> u64 {
        self.id
    }
    fn generation(&self) -> u32 {
        self.generation as u32
    }
    fn file_type(&self) -> FileType {
        self.kind
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn permission(
        &self,
        cred: &Cred,
        _: u32,
        attr: &Attr,
        acl: Option<&PosixAcl>,
        mask: u32,
    ) -> FsResult<()> {
        self.conn.allowed()?;
        if self.conn.default_permissions() {
            wasixv2_vfs::generic_permission_acl(cred, attr, acl, mask)
        } else if mask & wasixv2_vfs::MAY_EXEC != 0
            && self.kind == FileType::Regular
            && attr.mode & 0o111 == 0
        {
            Err(Errno::EACCES)
        } else {
            Ok(())
        }
    }
    async fn check_permission(
        &self,
        cred: &Cred,
        task: u32,
        attr: &Attr,
        acl: Option<&PosixAcl>,
        mask: u32,
    ) -> FsResult<()> {
        let result = self.permission(cred, task, attr, acl, mask);
        if self.conn.default_permissions() {
            if result == Err(Errno::EACCES) {
                let attr = self.refresh().await?;
                return self.permission(cred, task, &attr, acl, mask);
            }
            return result;
        }
        result?;
        // fuse_permission sends ACCESS only for access(2)/chdir, not each path component.
        if mask & (0x10 | 0x40) != 0 {
            match self
                .conn
                .optional(34, self.id, words32(&[mask & 7, 0]), 0, 0)
                .await
            {
                Ok(_) | Err(Errno::ENOSYS) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
    async fn getattr(&self) -> FsResult<Attr> {
        self.conn.allowed()?;
        {
            let cached = self.attr.lock_or_recover();
            if cached.1 > self.conn.host.monotonic() {
                return Ok(cached.0.clone());
            }
        }
        self.refresh().await
    }
    async fn setattr(&self, attr: &SetAttr) -> FsResult<Attr> {
        self.set(attr, None).await
    }
    async fn lookup(&self, name: &[u8]) -> FsResult<Arc<dyn Node>> {
        self.conn.allowed()?;
        {
            let mut entries = self.entries.lock_or_recover();
            if let Some(cached) = entries.get(name)
                && cached.until > self.conn.host.monotonic()
            {
                match &cached.node {
                    None => return Err(Errno::ENOENT),
                    Some(node) => {
                        if let Some(node) = node.upgrade() {
                            return Ok(node);
                        }
                    }
                }
            }
            entries.remove(name);
        }
        let mut body = Vec::new();
        super::wire::name(&mut body, name);
        let out = self.conn.request(1, self.id, body, 128, 128).await?;
        Ok(self.entry(name, &out)?)
    }
    async fn create(&self, name: &[u8], new: &NewNode<'_>) -> FsResult<Arc<dyn Node>> {
        if self.deleted.load(Ordering::Acquire) {
            return Err(Errno::ENOENT);
        }
        let (opcode, mut body) = match new.kind {
            NewKind::Directory => (9, words32(&[new.mode | self.kind.mode_bits(), 0])),
            NewKind::Symlink(_) => (6, Vec::new()),
            kind => {
                let rdev = match kind {
                    NewKind::CharDevice(d) | NewKind::BlockDevice(d) => {
                        (d.minor & 255) | (d.major << 8) | ((d.minor & !255) << 12)
                    }
                    _ => 0,
                };
                (
                    8,
                    words32(&[new.mode | kind.file_type().mode_bits(), rdev, 0, 0]),
                )
            }
        };
        super::wire::name(&mut body, name);
        if let NewKind::Symlink(target) = new.kind {
            super::wire::name(&mut body, target);
        }
        let out = self.conn.request(opcode, self.id, body, 128, 128).await?;
        self.invalidate();
        Ok(self.entry(name, &out)?)
    }
    async fn create_open(
        &self,
        name: &[u8],
        new: &NewNode<'_>,
        options: OpenOptions,
    ) -> FsResult<(Arc<dyn Node>, Box<dyn Handle>)> {
        let flags = Self::flags(options) | 0o100;
        let mut body = words32(&[flags, new.mode | FileType::Regular.mode_bits(), 0, 0]);
        super::wire::name(&mut body, name);
        match self.conn.optional(35, self.id, body, 144, 144).await {
            Ok(out) => {
                let node = match self.link_entry(name, &out[..128]) {
                    Ok(node) => node,
                    Err(error) => {
                        let id = u64_at(&out, 0)?;
                        let mut body = words64(&[u64_at(&out, 128)?]);
                        body.extend(words32(&[flags, 0]));
                        body.extend(words64(&[0]));
                        self.conn.enqueue_completion(
                            18,
                            id,
                            body,
                            0,
                            0,
                            super::dev::Completion::Forget(id),
                        );
                        return Err(error);
                    }
                };
                self.invalidate();
                let fh = u64_at(&out, 128)?;
                let open_flags = u32_at(&out, 136)?;
                let handle = FuseHandle {
                    node: node.clone(),
                    fh,
                    flags,
                    open_flags,
                    dir: false,
                    no_open: false,
                    poll: self.new_poll(),
                    cookies: Mutex::new(DirCookies::default()),
                };
                Ok((node, Box::new(handle)))
            }
            Err(Errno::ENOSYS) => {
                let node = self.create(name, new).await?;
                let handle = node.open(options).await?;
                Ok((node, handle))
            }
            Err(e) => Err(e),
        }
    }
    async fn readlink(&self) -> FsResult<Vec<u8>> {
        self.conn.request(5, self.id, Vec::new(), 0, 4096).await
    }
    async fn unlink(&self, name: &[u8]) -> FsResult<()> {
        self.remove(name, 10).await
    }
    async fn rmdir(&self, name: &[u8]) -> FsResult<()> {
        self.remove(name, 11).await
    }
    async fn link(&self, name: &[u8], target: &Arc<dyn Node>) -> FsResult<()> {
        let target = target.as_any().downcast_ref::<Self>().ok_or(Errno::EXDEV)?;
        let mut body = words64(&[target.id]);
        super::wire::name(&mut body, name);
        let out = self.conn.request(13, self.id, body, 128, 128).await?;
        self.entry(name, &out)?;
        target.invalidate();
        self.invalidate();
        Ok(())
    }
    async fn rename(
        &self,
        name: &[u8],
        new_dir: &Arc<dyn Node>,
        new_name: &[u8],
        mode: RenameMode,
    ) -> FsResult<()> {
        let dir = new_dir
            .as_any()
            .downcast_ref::<Self>()
            .ok_or(Errno::EXDEV)?;
        let flags = match mode {
            RenameMode::Replace => 0,
            RenameMode::NoReplace => 1,
            RenameMode::Exchange => 2,
            RenameMode::Whiteout => 4,
        };
        let opcode = if flags == 0 { 12 } else { 45 };
        let mut body = words64(&[dir.id]);
        if flags != 0 {
            body.extend(words32(&[flags, 0]));
        }
        super::wire::name(&mut body, name);
        super::wire::name(&mut body, new_name);
        self.conn.request(opcode, self.id, body, 0, 0).await?;
        self.invalidate_entry(name);
        dir.invalidate_entry(new_name);
        Ok(())
    }
    async fn open(&self, options: OpenOptions) -> FsResult<Box<dyn Handle>> {
        Ok(Box::new(self.open_handle(options).await?))
    }
    async fn getxattr(&self, name: &[u8]) -> FsResult<Vec<u8>> {
        let acl = AclKind::of_xattr(name).is_some();
        if acl && !self.acl_permissions() && !self.conn.user_ns.is_initial() {
            return Err(Errno::EOPNOTSUPP);
        }
        let mut body = words32(&[65536, 0]);
        super::wire::name(&mut body, name);
        let value = self
            .conn
            .optional(22, self.id, body, 0, 65536)
            .await
            .map_err(|e| {
                if e == Errno::ENOSYS {
                    Errno::EOPNOTSUPP
                } else {
                    e
                }
            })?;
        if acl {
            // fs/fuse/acl.c:__fuse_get_acl decodes ids in fc->user_ns. Drivers store
            // kernel ids for the VFS, which maps them to each syscall's user namespace.
            let value = if value.is_empty() {
                2u32.to_le_bytes().to_vec()
            } else {
                value
            };
            Ok(PosixAcl::from_xattr(
                &value,
                |id| self.conn.user_ns.make_kuid(id).map(|id| id.0),
                |id| self.conn.user_ns.make_kgid(id).map(|id| id.0),
            )?
            .map_or_else(|| 2u32.to_le_bytes().to_vec(), |a| a.to_stored()))
        } else {
            Ok(value)
        }
    }
    async fn listxattr(&self) -> FsResult<Vec<Vec<u8>>> {
        let out = self
            .conn
            .optional(23, self.id, words32(&[65536, 0]), 0, 65536)
            .await
            .map_err(|e| {
                if e == Errno::ENOSYS {
                    Errno::EOPNOTSUPP
                } else {
                    e
                }
            })?;
        if !out.is_empty() && out.last() != Some(&0) {
            return Err(Errno::EIO);
        }
        Ok(out
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .map(<[u8]>::to_vec)
            .collect())
    }
    async fn setxattr(&self, name: &[u8], value: &[u8], how: XattrSet) -> FsResult<()> {
        let encoded;
        let value = if AclKind::of_xattr(name).is_some() {
            if !self.acl_permissions() && !self.conn.user_ns.is_initial() {
                return Err(Errno::EOPNOTSUPP);
            }
            encoded = PosixAcl::from_stored(value)?.map_or_else(
                || 2u32.to_le_bytes().to_vec(),
                |a| {
                    a.to_xattr(
                        |id| self.conn.user_ns.map_kuid(Kuid(id)),
                        |id| self.conn.user_ns.map_kgid(Kgid(id)),
                    )
                },
            );
            &encoded
        } else {
            value
        };
        let flags = match how {
            XattrSet::Any => 0,
            XattrSet::Create => 1,
            XattrSet::Replace => 2,
        };
        let mut body = words32(&[value.len() as u32, flags]);
        super::wire::name(&mut body, name);
        body.extend(value);
        self.conn
            .optional(21, self.id, body, 0, 0)
            .await
            .map_err(|e| {
                if e == Errno::ENOSYS {
                    Errno::EOPNOTSUPP
                } else {
                    e
                }
            })?;
        self.invalidate();
        Ok(())
    }
    async fn removexattr(&self, name: &[u8]) -> FsResult<()> {
        let mut body = Vec::new();
        super::wire::name(&mut body, name);
        self.conn
            .optional(24, self.id, body, 0, 0)
            .await
            .map_err(|e| {
                if e == Errno::ENOSYS {
                    Errno::EOPNOTSUPP
                } else {
                    e
                }
            })?;
        self.invalidate();
        Ok(())
    }
}

/// FUSE offsets are opaque u64 values; wasm telldir is a signed 32-bit long.
#[derive(Default)]
struct DirCookies {
    from_wire: BTreeMap<u64, u64>,
    to_wire: BTreeMap<u64, u64>,
}
impl DirCookies {
    fn wire(&self, cookie: u64) -> FsResult<u64> {
        if cookie == 0 {
            Ok(0)
        } else {
            self.to_wire.get(&cookie).copied().ok_or(Errno::EINVAL)
        }
    }
    fn cookie(&mut self, wire: u64) -> FsResult<u64> {
        if let Some(cookie) = self.from_wire.get(&wire) {
            return Ok(*cookie);
        }
        let cookie = self.to_wire.len() as u64 + 1;
        if cookie > wasixv2_vfs::MAX_DIR_COOKIE {
            return Err(Errno::EOVERFLOW);
        }
        self.from_wire.insert(wire, cookie);
        self.to_wire.insert(cookie, wire);
        Ok(cookie)
    }
}

struct FuseHandle {
    node: Arc<FuseNode>,
    fh: u64,
    flags: u32,
    open_flags: u32,
    dir: bool,
    no_open: bool,
    poll: Arc<super::poll::PollState>,
    cookies: Mutex<DirCookies>,
}
impl Drop for FuseHandle {
    fn drop(&mut self) {
        self.node
            .conn
            .polls
            .lock_or_recover()
            .remove(&self.poll.key);
        // fs/fuse/file.c:fuse_file_open still allocates release_args for no_open
        // regular files; only no_opendir suppresses RELEASEDIR.
        if !self.no_open || !self.dir {
            self.node.conn.enqueue(
                if self.dir { 29 } else { 18 },
                self.node.id,
                {
                    let mut body = words64(&[self.fh]);
                    body.extend(words32(&[self.flags, 0]));
                    body.extend(words64(&[0]));
                    body
                },
                0,
                0,
            );
        }
    }
}
impl FuseHandle {
    fn poll_request(&self) {
        if !self.poll.stale()
            || self
                .poll
                .in_flight
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return;
        }
        let epoch = self.poll.epoch.load(Ordering::Acquire);
        if self.node.conn.unsupported.lock_or_recover().contains(&40) {
            self.poll.complete(epoch, wasixv2_wait::PollMask::DEFAULT.0);
            return;
        }
        let mut body = words64(&[self.fh, self.poll.key]);
        body.extend(words32(&[1, self.poll.events.load(Ordering::Acquire)]));
        self.node.conn.enqueue_completion(
            40,
            self.node.id,
            body,
            8,
            8,
            super::dev::Completion::Poll(Arc::downgrade(&self.poll), epoch),
        );
    }
    fn rw(&self, offset: u64, size: usize) -> Vec<u8> {
        let mut body = words64(&[self.fh, offset]);
        body.extend(words32(&[size as u32, 0]));
        body.extend(words64(&[0]));
        body.extend(words32(&[self.flags, 0]));
        body
    }
}
#[async_trait(?Send)]
impl Handle for FuseHandle {
    fn can_poll(&self) -> bool {
        !self.dir
    }
    async fn prepare_poll(&self, events: u32) {
        if self.dir {
            return;
        }
        self.poll.interest(events);
        let (_, _, _, signals) = self.node.conn.current();
        let no = wasixv2_wait::NoInterrupt;
        let interrupt: &dyn wasixv2_wait::Interrupt = signals.as_ref().map_or(&no, |s| &**s);
        loop {
            self.poll_request();
            if !self.poll.stale() && !self.poll.in_flight.load(Ordering::Acquire) {
                return;
            }
            let epoch = self.poll.completed.load(Ordering::Acquire);
            if wasixv2_wait::wait_event(
                &self.poll.wait,
                || {
                    (!self.poll.in_flight.load(Ordering::Acquire)
                        || self.poll.completed.load(Ordering::Acquire) != epoch)
                        .then_some(())
                },
                interrupt,
            )
            .await
            .is_err()
            {
                self.poll
                    .ready
                    .store(wasixv2_wait::PollMask::ERR.0, Ordering::Release);
                return;
            }
        }
    }
    fn poll(&self, mut table: Option<&mut wasixv2_wait::PollTable<'_>>) -> wasixv2_wait::PollMask {
        if self.dir {
            return wasixv2_wait::PollMask::DEFAULT;
        }
        let events = table
            .as_ref()
            .map_or(self.poll.events.load(Ordering::Acquire), |t| t.key().0);
        self.poll
            .interest(if events == 0 { u16::MAX as u32 } else { events });
        wasixv2_wait::poll_wait(&mut table, &self.poll.wait);
        self.poll_request();
        if self.poll.stale() {
            wasixv2_wait::PollMask::empty()
        } else {
            wasixv2_wait::PollMask(self.poll.ready.load(Ordering::Acquire))
        }
    }

    fn as_any(&self) -> Option<&dyn Any> {
        Some(self)
    }
    fn mmap(&self, shared: bool) -> FsResult<()> {
        // fuse_file_mmap rejects shared direct_io mappings without DIRECT_IO_ALLOW_MMAP.
        // The kernel's copied shared-file mappings cannot track daemon invalidations.
        if shared { Err(Errno::ENODEV) } else { Ok(()) }
    }
    fn flush_fatal(&self, owner: u64) {
        if !self.dir && self.open_flags & (1 << 5) == 0 {
            let mut body = words64(&[self.fh]);
            body.extend(words32(&[0, 0]));
            body.extend(words64(&[owner]));
            self.node.conn.detached_flush(self.node.id, body);
        }
    }
    async fn flush(&self, owner: u64) -> FsResult<()> {
        if self.dir || self.open_flags & (1 << 5) != 0 {
            return Ok(());
        }
        let mut body = words64(&[self.fh]);
        body.extend(words32(&[0, 0]));
        body.extend(words64(&[owner]));
        match self.node.conn.optional(25, self.node.id, body, 0, 0).await {
            Ok(_) | Err(Errno::ENOSYS) => Ok(()),
            Err(e) => Err(e),
        }
    }
    async fn copy_range(
        &self,
        offset: u64,
        output: &dyn Handle,
        output_offset: u64,
        len: u64,
    ) -> FsResult<Option<u64>> {
        let Some(dst) = output.as_any().and_then(|h| h.downcast_ref::<Self>()) else {
            return Ok(None);
        };
        if !Arc::ptr_eq(&self.node.conn, &dst.node.conn) {
            return Ok(None);
        }
        let body = words64(&[self.fh, offset, dst.node.id, dst.fh, output_offset, len, 0]);
        match self.node.conn.optional(47, self.node.id, body, 8, 8).await {
            Ok(out) => {
                let n = u64_at(&out, 0)?;
                if n > len {
                    return Err(Errno::EIO);
                }
                dst.node.invalidate();
                Ok(Some(n))
            }
            Err(Errno::ENOSYS) => Ok(None),
            Err(e) => Err(e),
        }
    }

    async fn read_at(&self, buf: &mut [u8], offset: u64) -> FsResult<usize> {
        let max = (self.node.conn.options.max_read.unwrap_or(MAX_IO as u32) as usize).min(MAX_IO);
        let mut done = 0;
        while done < buf.len() {
            let len = (buf.len() - done).min(max);
            let out = match self
                .node
                .conn
                .request(15, self.node.id, self.rw(offset + done as u64, len), 0, len)
                .await
            {
                Ok(out) => out,
                Err(_) if done != 0 => break,
                Err(e) => return Err(e),
            };
            buf[done..done + out.len()].copy_from_slice(&out);
            done += out.len();
            if out.len() < len {
                break;
            }
        }
        self.poll.notify();
        Ok(done)
    }
    async fn write_at(&self, data: &[u8], offset: u64) -> FsResult<usize> {
        self.node.conn.ready().await?;
        let max = self.node.conn.queue.lock_or_recover().max_write;
        let mut done = 0;
        while done < data.len() {
            let len = (data.len() - done).min(max);
            let mut body = self.rw(offset + done as u64, len);
            body.extend_from_slice(&data[done..done + len]);
            let out = match self.node.conn.request(16, self.node.id, body, 8, 8).await {
                Ok(out) => out,
                Err(_) if done != 0 => break,
                Err(e) => return Err(e),
            };
            let n = u32_at(&out, 0)? as usize;
            if n > len {
                if done != 0 {
                    break;
                }
                return Err(Errno::EIO);
            }
            done += n;
            if n < len {
                break;
            }
        }
        if done != 0 {
            self.node.invalidate();
            self.poll.notify();
        }
        Ok(done)
    }
    async fn append(&self, data: &[u8]) -> FsResult<(u64, usize)> {
        let _lock = self.node.data_lock.lock().await;
        let at = self.node.refresh().await?.size;
        let n = self.write_at(data, at).await?;
        Ok((at, n))
    }
    async fn sync(&self, data_only: bool) -> FsResult<()> {
        let mut body = words64(&[self.fh]);
        body.extend(words32(&[u32::from(data_only), 0]));
        match self
            .node
            .conn
            .optional(if self.dir { 30 } else { 20 }, self.node.id, body, 0, 0)
            .await
        {
            Ok(_) | Err(Errno::ENOSYS) => Ok(()),
            Err(e) => Err(e),
        }
    }
    async fn set_size(&self, size: u64) -> FsResult<()> {
        self.node
            .set(
                &SetAttr {
                    size: Some(size),
                    ..SetAttr::default()
                },
                Some(self.fh),
            )
            .await
            .map(|_| ())
    }
    async fn allocate(&self, mode: AllocMode, offset: u64, len: u64) -> FsResult<()> {
        let mode = match mode {
            AllocMode::Allocate => 0,
            AllocMode::AllocateKeepSize => 1,
            AllocMode::PunchHole => 3,
            AllocMode::ZeroRange { keep_size } => 16 | u32::from(keep_size),
        };
        let mut body = words64(&[self.fh, offset, len]);
        body.extend(words32(&[mode, 0]));
        self.node
            .conn
            .optional(43, self.node.id, body, 0, 0)
            .await
            .map_err(|e| {
                if e == Errno::ENOSYS {
                    Errno::EOPNOTSUPP
                } else {
                    e
                }
            })?;
        self.node.invalidate();
        Ok(())
    }
    async fn seek_hole(&self, offset: u64, size: u64, which: SeekHole) -> FsResult<u64> {
        let mut body = words64(&[self.fh, offset]);
        body.extend(words32(&[
            match which {
                SeekHole::Data => 3,
                SeekHole::Hole => 4,
            },
            0,
        ]));
        match self.node.conn.optional(46, self.node.id, body, 8, 8).await {
            Ok(out) => u64_at(&out, 0),
            Err(Errno::ENOSYS) => {
                if offset >= size {
                    Err(Errno::ENXIO)
                } else {
                    Ok(match which {
                        SeekHole::Data => offset,
                        SeekHole::Hole => size,
                    })
                }
            }
            Err(e) => Err(e),
        }
    }
    async fn readdir(&self, cookie: u64, sink: &mut dyn DirSink) -> FsResult<()> {
        let mut wire = self.cookies.lock_or_recover().wire(cookie)?;
        let mut seen = BTreeSet::new();
        loop {
            if !seen.insert(wire) {
                return Err(Errno::EIO);
            }
            let plus = self.node.conn.queue.lock_or_recover().flags & (1 << 13) != 0;
            let out = self
                .node
                .conn
                .request(
                    if plus { 44 } else { 28 },
                    self.node.id,
                    self.rw(wire, MAX_IO),
                    0,
                    MAX_IO,
                )
                .await?;
            if out.is_empty() {
                return Ok(());
            }
            let mut at = 0;
            let mut full = false;
            let mut progressed = false;
            let prefix = if plus { 128 } else { 0 };
            // fs/fuse/readdir.c:parse_dirplusfile links every entry, even after dir_emit
            // fills the caller's buffer. Unlinked entries must lose their lookup count.
            while out.len() - at >= prefix + 24 {
                let start = at;
                let d = at + prefix;
                let len = u32_at(&out, d + 16)? as usize;
                if len == 0 || len > 255 {
                    return Err(Errno::EIO);
                }
                let reclen = (prefix + 24 + len + 7) & !7;
                if reclen > out.len() - at {
                    break;
                }
                at += reclen;
                let name = &out[d + 24..d + 24 + len];
                if name.contains(&b'/') || name.contains(&0) {
                    return Err(Errno::EIO);
                }
                let next = u64_at(&out, d + 8)?;
                let ino = u64_at(&out, d)?;
                let dtype = u32_at(&out, d + 20)? as u8;
                if plus && name != b"." && name != b".." && u64_at(&out, start)? != 0 {
                    // A failed link already queues its matching FORGET; the directory
                    // record itself remains usable (fuse_direntplus_link).
                    let _ = self.node.entry(name, &out[start..start + 128]);
                }
                if full {
                    continue;
                }
                if name == b"." || name == b".." {
                    wire = next;
                    progressed = true;
                    continue;
                }
                let cookie = self.cookies.lock_or_recover().cookie(next)?;
                let kind = FileType::from_mode(u32::from(dtype) << 12).unwrap_or(FileType::Regular);
                if !sink.push_type(
                    DirEntry {
                        name,
                        ino,
                        file_type: kind,
                        next: cookie,
                    },
                    dtype,
                ) {
                    full = true;
                    continue;
                }
                wire = next;
                progressed = true;
            }
            if full {
                return Ok(());
            }
            // Linux ignores a partial trailing record; an entirely partial batch has no
            // further position to resume from, so it ends this directory read.
            if !progressed {
                return Ok(());
            }
        }
    }
}
