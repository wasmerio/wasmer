//! tmpfs's in-memory dquots (mm/shmem_quota.c), with fs/quota/dquot.c's limits,
//! grace periods and atomic user/group charges. Block limits in the quotactl UAPI
//! are KiB; the mount defaults and internal accounting are bytes.

use std::{collections::BTreeMap, sync::Mutex};

use wasixv2_abi::{Errno, IfDqblk, IfDqinfo, QuotaMisc as Q};
use wasixv2_vfs::{FsResult, QuotaOps};

use crate::{BLOCK, Shared};

const WEEK: u64 = 7 * 24 * 60 * 60;
const MAX_SPACE: u64 = i64::MAX as u64;

/// `shmem_options`'s quota types and default hard limits. `block_hard` is in bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QuotaLimits {
    pub types: u8,
    pub block_hard: [u64; 2],
    pub inode_hard: [u64; 2],
}

pub(crate) struct Quotas {
    pub(crate) defaults: QuotaLimits,
    kinds: Mutex<[Kind; 2]>,
}

struct Kind {
    active: bool,
    enforced: bool,
    info: IfDqinfo,
    ids: BTreeMap<u32, IfDqblk>,
    defaults: IfDqblk,
}

impl Kind {
    fn active(&self) -> FsResult<()> {
        self.active.then_some(()).ok_or(Errno::ESRCH)
    }

    fn id(&mut self, id: u32) -> &mut IfDqblk {
        self.ids.entry(id).or_insert(self.defaults)
    }

    /// `dquot_add_inodes`, `dquot_add_space`: try a charge without publishing it, so a
    /// failed group charge never leaves the user charged or starts its grace period.
    fn charged(
        &mut self,
        id: u32,
        bytes: u64,
        inodes: u64,
        now: u64,
        exempt: bool,
    ) -> FsResult<IfDqblk> {
        let enforced = self.enforced;
        let info = self.info;
        let mut next = *self.id(id);
        next.dqb_curspace = next.dqb_curspace.checked_add(bytes).ok_or(Errno::EDQUOT)?;
        next.dqb_curinodes = next
            .dqb_curinodes
            .checked_add(inodes)
            .ok_or(Errno::EDQUOT)?;
        if enforced {
            if bytes > 0 {
                charge_limit(
                    next.dqb_curspace,
                    next.dqb_bhardlimit,
                    next.dqb_bsoftlimit,
                    &mut next.dqb_btime,
                    info.dqi_bgrace,
                    now,
                    exempt,
                )?;
            }
            if inodes > 0 {
                charge_limit(
                    next.dqb_curinodes,
                    next.dqb_ihardlimit,
                    next.dqb_isoftlimit,
                    &mut next.dqb_itime,
                    info.dqi_igrace,
                    now,
                    exempt,
                )?;
            }
        }
        Ok(next)
    }

    fn release(&mut self, id: u32, bytes: u64, inodes: u64) {
        let dq = self.id(id);
        // Q_SETQUOTA can set usage below what the inode actually owns; Linux clamps frees.
        dq.dqb_curspace = dq.dqb_curspace.saturating_sub(bytes);
        dq.dqb_curinodes = dq.dqb_curinodes.saturating_sub(inodes);
        if dq.dqb_curspace <= dq.dqb_bsoftlimit {
            dq.dqb_btime = 0;
        }
        if dq.dqb_curinodes <= dq.dqb_isoftlimit {
            dq.dqb_itime = 0;
        }
    }
}

fn charge_limit(
    current: u64,
    hard: u64,
    soft: u64,
    time: &mut u64,
    grace: u64,
    now: u64,
    exempt: bool,
) -> FsResult<()> {
    if !exempt
        && ((hard != 0 && current > hard)
            || (soft != 0 && current > soft && *time != 0 && now >= *time))
    {
        return Err(Errno::EDQUOT);
    }
    if soft != 0 && current > soft && *time == 0 {
        *time = now.saturating_add(grace);
    }
    Ok(())
}

impl Quotas {
    pub(crate) fn new(defaults: QuotaLimits) -> Self {
        Self {
            defaults,
            kinds: Mutex::new(std::array::from_fn(|kind| Kind {
                active: defaults.types & (1 << kind) != 0,
                enforced: defaults.types & (1 << kind) != 0,
                info: IfDqinfo {
                    dqi_bgrace: WEEK,
                    dqi_igrace: WEEK,
                    dqi_flags: 1 << 16,
                    dqi_valid: Q::IIF_ALL.0,
                }, // DQF_SYS_FILE
                ids: BTreeMap::new(),
                defaults: IfDqblk {
                    dqb_bhardlimit: defaults.block_hard[kind],
                    dqb_ihardlimit: defaults.inode_hard[kind],
                    dqb_valid: Q::QIF_ALL.0,
                    ..IfDqblk::default()
                },
            })),
        }
    }

    pub(crate) fn charge(
        &self,
        owner: (u32, u32),
        bytes: u64,
        inodes: u64,
        now: u64,
        exempt: bool,
    ) -> FsResult<()> {
        let mut kinds = self.kinds.lock().unwrap();
        let mut next = [None; 2];
        for (kind, id) in [owner.0, owner.1].into_iter().enumerate() {
            if kinds[kind].active {
                next[kind] = Some(kinds[kind].charged(id, bytes, inodes, now, exempt)?);
            }
        }
        for (kind, id) in [owner.0, owner.1].into_iter().enumerate() {
            if let Some(dq) = next[kind] {
                kinds[kind].ids.insert(id, dq);
            }
        }
        Ok(())
    }

    pub(crate) fn release(&self, owner: (u32, u32), bytes: u64, inodes: u64) {
        let mut kinds = self.kinds.lock().unwrap();
        for (kind, id) in [owner.0, owner.1].into_iter().enumerate() {
            if kinds[kind].active {
                kinds[kind].release(id, bytes, inodes);
            }
        }
    }

    /// `__dquot_transfer`: both new owners must accept the whole inode before either old
    /// owner loses a charge. Unchanged ids neither lose usage nor check their limits again.
    pub(crate) fn transfer(
        &self,
        owners: [(u32, u32); 2],
        bytes: u64,
        now: u64,
        exempt: bool,
    ) -> FsResult<()> {
        let mut kinds = self.kinds.lock().unwrap();
        let [(uid, gid), (new_uid, new_gid)] = owners;
        let mut next = [None; 2];
        for (kind, (old, new)) in [(uid, new_uid), (gid, new_gid)].into_iter().enumerate() {
            if old != new && kinds[kind].active {
                next[kind] = Some(kinds[kind].charged(new, bytes, 1, now, exempt)?);
            }
        }
        for (kind, (old, new)) in [(uid, new_uid), (gid, new_gid)].into_iter().enumerate() {
            if let Some(dq) = next[kind] {
                kinds[kind].release(old, bytes, 1);
                kinds[kind].ids.insert(new, dq);
            }
        }
        Ok(())
    }
}

/// A single inode's allocation under its owner lock. Data operations use the same charge
/// point for quotas and the superblock's size limit (`shmem_alloc_folio`).
pub(crate) struct Charge<'a> {
    pub(crate) shared: &'a Shared,
    pub(crate) owner: (u32, u32),
    pub(crate) exempt: bool,
}

impl Charge<'_> {
    pub(crate) fn charge_blocks(&self, n: u64) -> FsResult<()> {
        self.shared.space.charge_blocks(n)?;
        if let Err(err) = self.shared.quotas.charge(
            self.owner,
            n * BLOCK,
            0,
            self.shared.now().sec.max(0) as u64,
            self.exempt,
        ) {
            self.shared.space.release_blocks(n);
            return Err(err);
        }
        Ok(())
    }

    pub(crate) fn release_blocks(&self, n: u64) {
        self.shared.quotas.release(self.owner, n * BLOCK, 0);
        self.shared.space.release_blocks(n);
    }
}

fn to_user(mut dq: IfDqblk) -> IfDqblk {
    dq.dqb_bhardlimit = dq.dqb_bhardlimit.div_ceil(1024);
    dq.dqb_bsoftlimit = dq.dqb_bsoftlimit.div_ceil(1024);
    dq
}

impl QuotaOps for crate::MemFs {
    fn active(&self, kind: usize) -> bool {
        self.shared
            .quotas
            .kinds
            .lock()
            .unwrap()
            .get(kind)
            .is_some_and(|k| k.active)
    }

    fn enforce(&self, kind: usize, enabled: bool) -> FsResult<()> {
        let mut kinds = self.shared.quotas.kinds.lock().unwrap();
        let kind = &mut kinds[kind];
        if enabled && !kind.active {
            return Err(Errno::EINVAL);
        }
        if kind.enforced == enabled {
            return Err(Errno::EEXIST);
        }
        kind.enforced = enabled;
        Ok(())
    }

    fn get(&self, kind: usize, id: u32) -> FsResult<IfDqblk> {
        let mut kinds = self.shared.quotas.kinds.lock().unwrap();
        kinds[kind].active()?;
        Ok(to_user(*kinds[kind].id(id)))
    }

    fn next(&self, kind: usize, id: u32) -> FsResult<(u32, IfDqblk)> {
        let kinds = self.shared.quotas.kinds.lock().unwrap();
        let kind = &kinds[kind];
        kind.active()?;
        // shmem_get_next_id skips records shmem_is_empty_dquot removes from its tree.
        kind.ids
            .range(id..)
            .find(|(_, dq)| {
                dq.dqb_curspace != 0
                    || dq.dqb_curinodes != 0
                    || dq.dqb_bhardlimit != kind.defaults.dqb_bhardlimit
                    || dq.dqb_ihardlimit != kind.defaults.dqb_ihardlimit
                    || dq.dqb_bsoftlimit != 0
                    || dq.dqb_isoftlimit != 0
            })
            .map(|(&id, &dq)| (id, to_user(dq)))
            .ok_or(Errno::ENOENT)
    }

    fn set(&self, kind: usize, id: u32, limits: &IfDqblk) -> FsResult<()> {
        let mut kinds = self.shared.quotas.kinds.lock().unwrap();
        let kind = &mut kinds[kind];
        kind.active()?;
        let valid = limits.dqb_valid;
        // copy_from_if_dqblk shifts UAPI KiB counts in u64 before checking the limits.
        let bhard = limits.dqb_bhardlimit.wrapping_shl(10);
        let bsoft = limits.dqb_bsoftlimit.wrapping_shl(10);
        // quota_setquota ignores unknown QIF bits (copy_from_if_dqblk).
        if valid & Q::QIF_BLIMITS.0 != 0 && (bhard > MAX_SPACE || bsoft > MAX_SPACE)
            || valid & Q::QIF_ILIMITS.0 != 0
                && (limits.dqb_ihardlimit > MAX_SPACE || limits.dqb_isoftlimit > MAX_SPACE)
        {
            return Err(Errno::ERANGE);
        }
        let info = kind.info;
        let now = self.shared.now().sec.max(0) as u64;
        let dq = kind.id(id);
        if valid & Q::QIF_BLIMITS.0 != 0 {
            dq.dqb_bhardlimit = bhard;
            dq.dqb_bsoftlimit = bsoft;
        }
        if valid & Q::QIF_ILIMITS.0 != 0 {
            dq.dqb_ihardlimit = limits.dqb_ihardlimit;
            dq.dqb_isoftlimit = limits.dqb_isoftlimit;
        }
        if valid & Q::QIF_SPACE.0 != 0 {
            dq.dqb_curspace = limits.dqb_curspace;
        }
        if valid & Q::QIF_INODES.0 != 0 {
            dq.dqb_curinodes = limits.dqb_curinodes;
        }
        if valid & Q::QIF_BTIME.0 != 0 {
            dq.dqb_btime = limits.dqb_btime;
        }
        if valid & Q::QIF_ITIME.0 != 0 {
            dq.dqb_itime = limits.dqb_itime;
        }
        if valid & (Q::QIF_BLIMITS.0 | Q::QIF_SPACE.0 | Q::QIF_BTIME.0) != 0 {
            if dq.dqb_bsoftlimit == 0 || dq.dqb_curspace <= dq.dqb_bsoftlimit {
                dq.dqb_btime = 0;
            } else if valid & Q::QIF_BTIME.0 == 0 {
                dq.dqb_btime = now.saturating_add(info.dqi_bgrace);
            }
        }
        if valid & (Q::QIF_ILIMITS.0 | Q::QIF_INODES.0 | Q::QIF_ITIME.0) != 0 {
            if dq.dqb_isoftlimit == 0 || dq.dqb_curinodes <= dq.dqb_isoftlimit {
                dq.dqb_itime = 0;
            } else if valid & Q::QIF_ITIME.0 == 0 {
                dq.dqb_itime = now.saturating_add(info.dqi_igrace);
            }
        }
        Ok(())
    }

    fn info(&self, kind: usize) -> FsResult<IfDqinfo> {
        let kinds = self.shared.quotas.kinds.lock().unwrap();
        kinds[kind].active()?;
        Ok(kinds[kind].info)
    }

    fn set_info(&self, kind: usize, info: &IfDqinfo) -> FsResult<()> {
        // quota_setinfo, dquot_set_dqinfo: ROOT_SQUASH only applies to the old format.
        if info.dqi_valid & !Q::IIF_ALL.0 != 0
            || info.dqi_valid & Q::IIF_FLAGS.0 != 0 && info.dqi_flags != 0
        {
            return Err(Errno::EINVAL);
        }
        let mut kinds = self.shared.quotas.kinds.lock().unwrap();
        let kind = &mut kinds[kind];
        kind.active()?;
        if info.dqi_valid & Q::IIF_BGRACE.0 != 0 {
            kind.info.dqi_bgrace = info.dqi_bgrace;
        }
        if info.dqi_valid & Q::IIF_IGRACE.0 != 0 {
            kind.info.dqi_igrace = info.dqi_igrace;
        }
        Ok(())
    }
}
