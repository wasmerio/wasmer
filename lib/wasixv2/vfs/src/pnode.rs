//! Mount propagation (Linux's `fs/pnode.c`) and the tree copies it is made of (`clone_mnt`,
//! `copy_tree`, `umount_tree` in `fs/namespace.c`).
//!
//! Propagation is kept per peer group: a shared mount carries its group's id (`mnt_group_id`),
//! and a slave the id of the group it receives from (`mnt_master`, whose peers all have the
//! same master, so the group stands for it). A mount's peers are the mounts with its group id,
//! its slaves the mounts whose master is its group. Events go from a mount to its peers and,
//! through its group, to its slaves, their peers and their slaves in turn. What propagates is a
//! mount attached on a directory of a shared mount, with everything mounted below it, and the
//! unmounting of such a mount where nothing is mounted on its copies.

use std::{
    collections::HashSet,
    sync::{
        Arc, Weak,
        atomic::{AtomicU64, Ordering},
    },
};

use wasixv2_abi::Errno;

use crate::{
    dentry::Dentry,
    mount::{Mount, Path, TreeState},
    types::FsResult,
};

/// A mount's place in its namespace and in propagation.
#[derive(Clone)]
pub(crate) struct Link {
    pub(crate) mount: Weak<Mount>,
    /// The namespace the mount is attached in; 0 while it is part of a tree being built
    /// (`IS_MNT_NEW`).
    pub(crate) ns: u64,
    /// Its peer group if it is shared (`mnt_group_id`), else 0.
    pub(crate) group: u32,
    /// The peer group it receives from if it is a slave (`mnt_master`), else 0.
    pub(crate) master: u32,
    /// `T_UNBINDABLE`.
    pub(crate) unbindable: bool,
}

/// Each mount a tree copy copied, with its copy.
pub(crate) type Copies = Vec<(Arc<Mount>, Arc<Mount>)>;

/// A propagation type (`MS_SHARED`, `MS_SLAVE`, `MS_PRIVATE`, `MS_UNBINDABLE`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Propagation {
    Shared,
    Slave,
    Private,
    Unbindable,
}

/// How a mount is copied (`CL_*`).
#[derive(Clone, Copy, Default)]
pub(crate) struct CopyFlags {
    /// `CL_SLAVE`: the copy receives from the original.
    pub(crate) slave: bool,
    /// `CL_PRIVATE`: the copy has no propagation at all.
    pub(crate) private: bool,
    /// `CL_MAKE_SHARED`: the copy is shared, in the original's group if it has one.
    pub(crate) make_shared: bool,
    /// `CL_COPY_UNBINDABLE`: unbindable mounts are copied too, not left out.
    pub(crate) copy_unbindable: bool,
}

impl TreeState {
    fn link(&self, id: u64) -> &Link {
        self.links
            .get(&id)
            .expect("every mount in a tree is linked")
    }

    fn link_mut(&mut self, id: u64) -> &mut Link {
        self.links
            .get_mut(&id)
            .expect("every mount in a tree is linked")
    }

    /// Records a mount that a namespace's tree is about to hold.
    pub(crate) fn add_link(&mut self, mount: &Arc<Mount>, ns: u64) {
        self.links.insert(
            mount.id,
            Link {
                mount: Arc::downgrade(mount),
                ns,
                group: 0,
                master: 0,
                unbindable: false,
            },
        );
    }

    pub(crate) fn set_ns(&mut self, mounts: &[Arc<Mount>], ns: u64) {
        for mount in mounts {
            self.link_mut(mount.id).ns = ns;
        }
    }

    pub(crate) fn group_of(&self, mount: &Mount) -> u32 {
        self.links.get(&mount.id).map_or(0, |link| link.group)
    }

    pub(crate) fn is_shared(&self, mount: &Mount) -> bool {
        self.group_of(mount) != 0
    }

    pub(crate) fn is_unbindable(&self, mount: &Mount) -> bool {
        self.links
            .get(&mount.id)
            .is_some_and(|link| link.unbindable)
    }

    /// The propagation of `mount`: its group, master and whether it is unbindable.
    pub(crate) fn propagation_of(&self, mount: &Mount) -> (u32, u32, bool) {
        self.links.get(&mount.id).map_or((0, 0, false), |link| {
            (link.group, link.master, link.unbindable)
        })
    }

    /// The lowest group id no mount uses (`mnt_alloc_group_id`, an IDA from 1).
    fn alloc_group(&self) -> u32 {
        let used: HashSet<u32> = self
            .links
            .values()
            .flat_map(|link| [link.group, link.master])
            .collect();
        (1..)
            .find(|id| !used.contains(id))
            .expect("u32 ids suffice")
    }

    /// `invent_group_ids`: a new group for each of `mounts` that has none. Returns the mounts
    /// that got one, for `cleanup_group_ids`.
    pub(crate) fn invent_group_ids(&mut self, mounts: &[Arc<Mount>]) -> Vec<u64> {
        let mut invented = Vec::new();
        for mount in mounts {
            if self.link(mount.id).group == 0 {
                let group = self.alloc_group();
                self.link_mut(mount.id).group = group;
                invented.push(mount.id);
            }
        }
        invented
    }

    /// `cleanup_group_ids`.
    pub(crate) fn cleanup_group_ids(&mut self, invented: &[u64]) {
        for id in invented {
            if let Some(link) = self.links.get_mut(id) {
                link.group = 0;
            }
        }
    }

    /// The mounts of group `group` in a namespace, oldest first (the `mnt_share` ring).
    fn members(&self, group: u32) -> Vec<Arc<Mount>> {
        self.collect(|link| link.group == group && link.ns != 0)
    }

    /// The mounts in a namespace that receive from group `group` (its `mnt_slave_list`).
    fn slaves(&self, group: u32) -> Vec<Arc<Mount>> {
        self.collect(|link| link.master == group && link.ns != 0)
    }

    fn collect(&self, keep: impl Fn(&Link) -> bool) -> Vec<Arc<Mount>> {
        let mut found: Vec<_> = self
            .links
            .values()
            .filter(|link| keep(link))
            .filter_map(|link| link.mount.upgrade())
            .collect();
        found.sort_by_key(|mount| mount.id);
        found
    }

    /// The group that group `group` receives from: its members' master.
    fn group_master(&self, group: u32) -> u32 {
        self.links
            .values()
            .find(|link| link.group == group)
            .map_or(0, |link| link.master)
    }

    /// `change_mnt_propagation`. A mount leaving its group hands the group's slaves on to its
    /// master if it was the last member (`transfer_propagation`); a mount made a slave
    /// receives from the group it leaves, or from its master if it was alone.
    pub(crate) fn change_propagation(&mut self, id: u64, kind: Propagation) {
        let Link { group, master, .. } = *self.link(id);
        if kind == Propagation::Shared {
            if group == 0 {
                let group = self.alloc_group();
                self.link_mut(id).group = group;
            }
            self.link_mut(id).unbindable = false;
            return;
        }
        let mut new_master = master;
        if group != 0 {
            let others = self
                .links
                .iter()
                .any(|(&other, link)| other != id && link.group == group);
            if others {
                new_master = group;
            } else {
                for link in self.links.values_mut() {
                    if link.master == group {
                        link.master = master;
                    }
                }
            }
            self.link_mut(id).group = 0;
        }
        let link = self.link_mut(id);
        match kind {
            Propagation::Slave => link.master = new_master,
            Propagation::Private | Propagation::Unbindable => {
                link.master = 0;
                link.unbindable = kind == Propagation::Unbindable;
            }
            Propagation::Shared => unreachable!("handled above"),
        }
    }

    /// `clone_mnt`: a copy of `old` starting at `root`, attached nowhere.
    pub(crate) fn clone_mnt(
        &mut self,
        ids: &AtomicU64,
        old: &Arc<Mount>,
        root: Arc<Dentry>,
        flags: CopyFlags,
    ) -> Arc<Mount> {
        let mount = Arc::new(Mount::new(
            ids.fetch_add(1, Ordering::Relaxed),
            root,
            old.flags(),
            old.devname().to_vec(),
        ));
        let (old_group, old_master, unbindable) = self.propagation_of(old);
        let mut group = match flags.slave || flags.private {
            true => 0,
            false => old_group,
        };
        if flags.make_shared && group == 0 {
            group = self.alloc_group();
        }
        let master = if flags.private {
            0
        } else if flags.slave && old_group != 0 {
            old_group
        } else {
            old_master
        };
        self.links.insert(
            mount.id,
            Link {
                mount: Arc::downgrade(&mount),
                ns: 0,
                group,
                master,
                unbindable,
            },
        );
        mount
    }

    /// `copy_tree`: a copy of `src` starting at `dentry`, with copies of the mounts below it
    /// that `dentry` reaches attached on the copies of their parents. Returns the copy and,
    /// for each original copied, its copy.
    pub(crate) fn copy_tree(
        &mut self,
        ids: &AtomicU64,
        src: &Arc<Mount>,
        dentry: &Arc<Dentry>,
        flags: CopyFlags,
    ) -> FsResult<(Arc<Mount>, Copies)> {
        if !flags.copy_unbindable && self.is_unbindable(src) {
            return Err(Errno::EINVAL);
        }
        let top = self.clone_mnt(ids, src, dentry.clone(), flags);
        let mut copies = vec![(src.clone(), top.clone())];
        let mut pending: Vec<(Arc<Mount>, Arc<Mount>)> = self
            .children_of(src)
            .into_iter()
            .filter(|child| {
                child
                    .parent()
                    .is_some_and(|at| dentry.is_ancestor_of(&at.dentry))
            })
            .map(|child| (child, top.clone()))
            .collect();
        pending.reverse();
        while let Some((original, parent_copy)) = pending.pop() {
            if !flags.copy_unbindable && self.is_unbindable(&original) {
                continue;
            }
            let Some(at) = original.parent() else {
                continue;
            };
            let copy = self.clone_mnt(ids, &original, original.root.clone(), flags);
            self.attach(
                &copy,
                &Path {
                    mnt: parent_copy,
                    dentry: at.dentry,
                },
            );
            let mut below: Vec<_> = self
                .children_of(&original)
                .into_iter()
                .map(|child| (child, copy.clone()))
                .collect();
            below.reverse();
            pending.extend(below);
            copies.push((original, copy));
        }
        Ok((top, copies))
    }

    /// `umount_tree`: takes `mount` and everything below it out of their tree, and with
    /// `propagate` their copies that propagation made where nothing else is mounted on them
    /// (`propagate_umount`). The mounts lose their propagation (`bulk_make_private`) and their
    /// parents; what still holds one keeps a detached tree. Returns them, to be dropped after
    /// the table's lock.
    pub(crate) fn umount_tree(&mut self, mount: &Arc<Mount>, propagate: bool) -> Vec<Arc<Mount>> {
        let mut victims = self.subtree(mount);
        if propagate {
            self.propagate_umount(&mut victims);
        }
        for victim in &victims {
            if self.links.contains_key(&victim.id) {
                self.change_propagation(victim.id, Propagation::Private);
            }
        }
        for victim in &victims {
            self.detach(victim);
            self.links.remove(&victim.id);
        }
        victims
    }

    /// The mounts in a namespace that get what is mounted on `parent` (`receives_from` over
    /// the propagation tree): its peers, their slaves, the slaves' peers and so on.
    fn receivers(&self, parent: &Mount) -> Vec<Arc<Mount>> {
        let group = self.group_of(parent);
        if group == 0 {
            return Vec::new();
        }
        self.collect(|link| {
            link.ns != 0
                && !std::ptr::eq(link.mount.as_ptr(), parent)
                && self.receives_from_group(link, group)
        })
    }

    fn receives_from_group(&self, link: &Link, group: u32) -> bool {
        if link.group == group {
            return true;
        }
        let mut master = link.master;
        // Masters form a tree; the bound only guards against a broken one.
        for _ in 0..64 {
            if master == 0 {
                return false;
            }
            if master == group {
                return true;
            }
            master = self.group_master(master);
        }
        false
    }

    /// `need_secondary`: whether `mount`, a receiver, gets a copy of a mount on `mountpoint`:
    /// it is in a namespace and its tree shows the mountpoint.
    fn need_secondary(&self, mount: &Mount, mountpoint: &Arc<Dentry>) -> bool {
        self.links.get(&mount.id).is_some_and(|link| link.ns != 0)
            && mount.root.is_ancestor_of(mountpoint)
    }

    /// `propagate_mnt`: copies of the tree at `source`, about to be attached on `mountpoint`
    /// of the shared mount `dest`, for every receiver of `dest`, each with where it goes. A
    /// peer's copy is a peer of `source`; a slave's copy receives from the copy made for its
    /// master, and the copies for a group of slaves are peers.
    pub(crate) fn propagate_mnt(
        &mut self,
        ids: &AtomicU64,
        dest: &Arc<Mount>,
        mountpoint: &Arc<Dentry>,
        source: &Arc<Mount>,
    ) -> FsResult<Vec<(Path, Arc<Mount>)>> {
        let group = self.group_of(dest);
        let mut out = Vec::new();
        if group == 0 {
            return Ok(out);
        }
        let shared = CopyFlags {
            make_shared: true,
            ..CopyFlags::default()
        };
        for peer in self.members(group) {
            if Arc::ptr_eq(&peer, dest) || !self.need_secondary(&peer, mountpoint) {
                continue;
            }
            let (copy, _) = self.copy_tree(ids, source, &source.root, shared)?;
            out.push((at(&peer, mountpoint), copy));
        }
        let mut seen = HashSet::from([group]);
        self.propagate_to_slaves(ids, group, source, mountpoint, &mut seen, &mut out)?;
        Ok(out)
    }

    fn propagate_to_slaves(
        &mut self,
        ids: &AtomicU64,
        group: u32,
        master_copy: &Arc<Mount>,
        mountpoint: &Arc<Dentry>,
        seen: &mut HashSet<u32>,
        out: &mut Vec<(Path, Arc<Mount>)>,
    ) -> FsResult<()> {
        for slave in self.slaves(group) {
            let slave_group = self.group_of(&slave);
            if slave_group == 0 {
                if self.need_secondary(&slave, mountpoint) {
                    let flags = CopyFlags {
                        slave: true,
                        ..CopyFlags::default()
                    };
                    let (copy, _) = self.copy_tree(ids, master_copy, &master_copy.root, flags)?;
                    out.push((at(&slave, mountpoint), copy));
                }
                continue;
            }
            if !seen.insert(slave_group) {
                continue;
            }
            let mut first: Option<Arc<Mount>> = None;
            for member in self.members(slave_group) {
                if !self.need_secondary(&member, mountpoint) {
                    continue;
                }
                let (from, flags) = match &first {
                    None => (
                        master_copy.clone(),
                        CopyFlags {
                            slave: true,
                            make_shared: true,
                            ..CopyFlags::default()
                        },
                    ),
                    Some(first) => (
                        first.clone(),
                        CopyFlags {
                            make_shared: true,
                            ..CopyFlags::default()
                        },
                    ),
                };
                let (copy, _) = self.copy_tree(ids, &from, &from.root, flags)?;
                out.push((at(&member, mountpoint), copy.clone()));
                first.get_or_insert(copy);
            }
            let next = first.unwrap_or_else(|| master_copy.clone());
            self.propagate_to_slaves(ids, slave_group, &next, mountpoint, seen, out)?;
        }
        Ok(())
    }

    /// `propagate_umount` for `victims` (a subtree, parents first): adds the mounts that
    /// receivers of each victim's parent have on the same mountpoint, if nothing but other
    /// victims is mounted on them. Deepest victims go first, so a copy of a whole subtree goes
    /// with it.
    fn propagate_umount(&self, victims: &mut Vec<Arc<Mount>>) {
        let originals: Vec<_> = victims.iter().rev().cloned().collect();
        for victim in originals {
            let Some(at) = victim.parent() else {
                continue;
            };
            for receiver in self.receivers(&at.mnt) {
                if victims.iter().any(|v| Arc::ptr_eq(v, &receiver)) {
                    continue;
                }
                let Some(copy) = self.mounted_on(&self::at(&receiver, &at.dentry)) else {
                    continue;
                };
                if victims.iter().any(|v| Arc::ptr_eq(v, &copy)) {
                    continue;
                }
                let covered = self
                    .children_of(&copy)
                    .iter()
                    .any(|child| !victims.iter().any(|v| Arc::ptr_eq(v, child)));
                if !covered {
                    victims.push(copy);
                }
            }
        }
    }

    /// `propagate_mount_busy`: `mount` has something mounted on it or more references than
    /// `refs`, or a copy that propagation would unmount with it is in use.
    pub(crate) fn propagate_mount_busy(&self, mount: &Arc<Mount>, refs: usize) -> bool {
        if self.has_children(mount) || Arc::strong_count(mount) > refs {
            return true;
        }
        let Some(at) = mount.parent() else {
            return false;
        };
        self.receivers(&at.mnt).iter().any(|receiver| {
            self.mounted_on(&self::at(receiver, &at.dentry))
                .is_some_and(|copy| {
                    // The table's reference and the one just taken.
                    !self.has_children(&copy) && Arc::strong_count(&copy) > 2
                })
        })
    }

    /// `get_dominating_id`: the closest group up `mount`'s chain of masters that has a member
    /// in the same namespace under `root`, for `mountinfo`'s `propagate_from`.
    pub(crate) fn dominating_group(&self, mount: &Mount, root: &Path) -> u32 {
        let Some(link) = self.links.get(&mount.id) else {
            return 0;
        };
        let mut master = link.master;
        for _ in 0..64 {
            if master == 0 {
                break;
            }
            let reachable = self.members(master).iter().any(|member| {
                self.link(member.id).ns == link.ns
                    && crate::mount::is_path_reachable(member, &member.root, root)
            });
            if reachable {
                return master;
            }
            master = self.group_master(master);
        }
        0
    }
}

fn at(mount: &Arc<Mount>, dentry: &Arc<Dentry>) -> Path {
    Path {
        mnt: mount.clone(),
        dentry: dentry.clone(),
    }
}
