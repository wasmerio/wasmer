//! Mount propagation (Linux's `fs/pnode.c`) and the tree copies it is made of (`clone_mnt`,
//! `copy_tree`, `umount_tree` in `fs/namespace.c`).
//!
//! Propagation is kept per peer group: a shared mount carries its group's id (`mnt_group_id`),
//! and a slave the id of the group it receives from (`mnt_master`, whose peers all have the
//! same master, so the group stands for it). A mount's peers are the mounts with its group id,
//! its slaves the mounts whose master is its group. Events go from a mount to its peers and,
//! through its group, to its slaves, their peers and their slaves in turn. What propagates is a
//! mount attached on a directory of a shared mount, with everything mounted below it, and the
//! unmounting of such a mount with its whole tree: a copy goes unless something it does not
//! take along is mounted on it, and what is mounted on the root of a copy that goes takes its
//! place (`propagate_umount`, `Documentation/filesystems/propagate_umount.txt`).

use std::{
    collections::{HashSet, VecDeque},
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

    /// `do_set_group`'s change: `mount` joins `group` and receives from `master`.
    pub(crate) fn set_propagation(&mut self, mount: &Mount, group: u32, master: u32) {
        let link = self.link_mut(mount.id);
        link.group = group;
        link.master = master;
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
    /// `propagate` the copies propagation made of them that can go (`propagate_umount`). The
    /// mounts lose their propagation (`bulk_make_private`) and their parents; what still holds
    /// one keeps a detached tree. A mount on the root of a copy that goes, which does not go
    /// itself, takes the place of the stack it was on. Returns the mounts, to be dropped after
    /// the table's lock.
    pub(crate) fn umount_tree(&mut self, mount: &Arc<Mount>, propagate: bool) -> Vec<Arc<Mount>> {
        let mut victims = self.subtree(mount);
        let mut reparent = Vec::new();
        if propagate {
            reparent = self.propagate_umount(&mut victims);
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
        for (over, place) in reparent {
            self.detach(&over);
            self.attach(&over, &place);
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
    /// it is in a namespace that is not a detached tree's, and its tree shows the mountpoint.
    fn need_secondary(&self, mount: &Mount, mountpoint: &Arc<Dentry>) -> bool {
        self.links
            .get(&mount.id)
            .is_some_and(|link| link.ns != 0 && !self.anon_ns.contains_key(&link.ns))
            && mount.root.is_ancestor_of(mountpoint)
    }

    /// `propagation_would_overmount`: whether a mount attached on `mountpoint` of the shared
    /// mount `from` would have a copy put on top of `to`, whose root is that mountpoint: `to`
    /// is a peer of `from` or receives from its group.
    pub(crate) fn propagation_would_overmount(
        &self,
        from: &Mount,
        to: &Mount,
        mountpoint: &Arc<Dentry>,
    ) -> bool {
        let group = self.group_of(from);
        if group == 0 || !Arc::ptr_eq(&to.root, mountpoint) {
            return false;
        }
        self.links
            .get(&to.id)
            .is_some_and(|link| self.receives_from_group(link, group))
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

    /// `receives_from`: `mount` gets what is mounted on `parent`, a mount other than itself.
    fn receives_from(&self, mount: &Mount, parent: &Mount) -> bool {
        let group = self.group_of(parent);
        group != 0
            && mount.id != parent.id
            && self
                .links
                .get(&mount.id)
                .is_some_and(|link| self.receives_from_group(link, group))
    }

    /// The mount on the root of `mount` (`overmount`).
    fn overmount(&self, mount: &Arc<Mount>) -> Option<Arc<Mount>> {
        self.mounted_on(&mount.root_path())
    }

    /// `propagate_umount` for `set`, a subtree being unmounted, parents first: adds the
    /// mounts that receivers of a member's parent have on its mountpoint (the candidates) and
    /// that can go with it. A candidate goes when nothing but other candidates going too, or a
    /// mount on its root, is mounted on it; a mount on its root that stays takes the place of
    /// the stack it was on. Returns each such mount with that place.
    fn propagate_umount(&self, set: &mut Vec<Arc<Mount>>) -> Vec<(Arc<Mount>, Path)> {
        let mut umount = Umount {
            unmounted: set.iter().map(|m| m.id).collect(),
            ..Umount::default()
        };
        self.gather_candidates(set, &mut umount);
        // Reduce the candidates to those that do not shift what stays.
        let pending: Vec<_> = umount.list.iter().cloned().collect();
        for candidate in pending {
            if umount.list.iter().any(|m| Arc::ptr_eq(m, &candidate)) {
                self.trim_one(&candidate, &mut umount);
            }
        }
        // What is left has only candidates below it: it goes with them.
        while let Some(first) = umount.list.front().cloned() {
            self.handle_rest(&first, &mut umount);
        }
        let mut reparent = Vec::new();
        for gone in &umount.to_umount {
            if let Some(over) = self.overmount(gone)
                && !umount.unmounted.contains(&over.id)
            {
                reparent.push((over.clone(), self.below_unmounted(&over, &umount)));
            }
        }
        set.extend(umount.to_umount);
        reparent
    }

    /// `gather_candidates`.
    fn gather_candidates(&self, set: &[Arc<Mount>], umount: &mut Umount) {
        for member in set {
            if !umount.candidate.insert(member.id) {
                continue;
            }
            let Some(at) = member.parent() else {
                continue;
            };
            for receiver in self.receivers(&at.mnt) {
                let Some(child) = self.mounted_on(&self::at(&receiver, &at.dentry)) else {
                    continue;
                };
                if !umount.candidate.insert(child.id) {
                    continue;
                }
                if !umount.unmounted.contains(&child.id) {
                    umount.list.push_front(child);
                }
            }
        }
        for member in set {
            umount.candidate.remove(&member.id);
        }
    }

    /// The mounts on `mount` that are not going already (its `mnt_mounts`, from which
    /// unmounted mounts are hidden).
    fn staying_children(&self, mount: &Mount, umount: &Umount) -> Vec<Arc<Mount>> {
        self.children_of(mount)
            .into_iter()
            .filter(|child| !umount.unmounted.contains(&child.id))
            .collect()
    }

    /// `trim_one`: drops the candidates `mount` rules out, and decides on `mount` itself if it
    /// can: it goes if nothing is mounted on it, and stays if something that stays is mounted
    /// on it other than on its root.
    fn trim_one(&self, mount: &Arc<Mount>, umount: &mut Umount) {
        if !umount.candidate.contains(&mount.id) {
            umount.remove_from_list(mount);
            return;
        }
        let over = self.overmount(mount);
        let children = self.staying_children(mount, umount);
        let mut found = false;
        let mut remove_this = false;
        let mut umount_this = false;
        for child in &children {
            if !umount.candidate.contains(&child.id) {
                found = true;
                if over.as_ref().is_none_or(|over| !Arc::ptr_eq(over, child)) {
                    remove_this = true;
                    break;
                }
            }
        }
        if found {
            self.trim_ancestors(mount, umount);
        } else if children.is_empty() {
            remove_this = true;
            umount_this = true;
        }
        if remove_this {
            umount.remove_from_list(mount);
            if umount_this {
                umount.umount_one(mount);
            }
        }
    }

    /// `trim_ancestors`: something under `mount` stays, so each candidate above it that it is
    /// not mounted on the root of stays too.
    fn trim_ancestors(&self, mount: &Arc<Mount>, umount: &mut Umount) {
        let mut mount = mount.clone();
        while let Some(at) = mount.parent()
            && umount.candidate.contains(&at.mnt.id)
        {
            // Every candidate above was looked at from here already.
            if !umount.marked.insert(mount.id) {
                return;
            }
            let on_root = self
                .overmount(&at.mnt)
                .is_some_and(|over| Arc::ptr_eq(&over, &mount));
            if !on_root {
                umount.candidate.remove(&at.mnt.id);
            }
            mount = at.mnt;
        }
    }

    /// `handle_locked` without locked mounts: a candidate still undecided has only
    /// candidates under it, so it goes with the chain of candidates above it.
    fn handle_rest(&self, mount: &Arc<Mount>, umount: &mut Umount) {
        if !umount.candidate.contains(&mount.id) {
            umount.remove_from_list(mount);
            return;
        }
        let mut chain = vec![mount.clone()];
        while let Some(at) = chain.last().and_then(|m| m.parent())
            && umount.candidate.contains(&at.mnt.id)
        {
            chain.push(at.mnt);
        }
        for member in &chain {
            umount.remove_from_list(member);
        }
        for member in &chain {
            umount.umount_one(member);
        }
    }

    /// Where `over`, on top of a stack of mounts that go, goes instead: where the bottom of
    /// the stack is attached (`reparent`).
    fn below_unmounted(&self, over: &Arc<Mount>, umount: &Umount) -> Path {
        let mut place = over.parent().expect("an overmount is attached");
        while umount.unmounted.contains(&place.mnt.id)
            && let Some(below) = place.mnt.parent()
        {
            place = below;
        }
        place
    }

    /// Whether `mount` has references besides the table's, its children's and `held` the
    /// caller holds.
    fn has_users(&self, mount: &Arc<Mount>, held: usize) -> bool {
        let children = self.children_of(mount).len();
        Arc::strong_count(mount) > 1 + children + held
    }

    /// `propagate_mount_busy`: `mount` has something mounted on it or more references than
    /// `refs`, or a mount that propagation would unmount with it is in use.
    pub(crate) fn propagate_mount_busy(&self, mount: &Arc<Mount>, refs: usize) -> bool {
        if self.has_children(mount) || Arc::strong_count(mount) > refs {
            return true;
        }
        let Some(at) = mount.parent() else {
            return false;
        };
        let mut firsts = Vec::new();
        for receiver in self.receivers(&at.mnt) {
            let Some(candidate) = self.mounted_on(&self::at(&receiver, &at.dentry)) else {
                continue;
            };
            // Each chain once, from its top: a receiver that is a candidate itself is in the
            // chain of the one it is mounted on.
            let in_chain = receiver.parent().is_some_and(|up| {
                Arc::ptr_eq(&up.dentry, &at.dentry) && self.receives_from(&up.mnt, &at.mnt)
            });
            if !in_chain {
                firsts.push(candidate);
            }
        }
        // The parent is a candidate itself when it receives from its own peers (a shared mount
        // moved below a peer of itself): its counts must not include this reference.
        drop(at);
        firsts
            .iter()
            .any(|first| self.chain_busy(first, mount, &firsts))
    }

    /// `next_candidate`: the mount at `victim`'s mountpoint on `mount`, if `mount` receives
    /// from `victim`'s parent.
    fn next_candidate(&self, mount: &Arc<Mount>, victim: &Arc<Mount>) -> Option<Arc<Mount>> {
        let at = victim.parent()?;
        if !self.receives_from(mount, &at.mnt) {
            return None;
        }
        self.mounted_on(&self::at(mount, &at.dentry))
    }

    /// `chain_busy`: whether unmounting `victim` would take out a mount of the chain of
    /// candidates starting at `first` that something uses. `held` are references the caller
    /// holds.
    fn chain_busy(&self, first: &Arc<Mount>, victim: &Arc<Mount>, held: &[Arc<Mount>]) -> bool {
        let is =
            |a: &Arc<Mount>, b: &Option<Arc<Mount>>| b.as_ref().is_some_and(|b| Arc::ptr_eq(a, b));
        // The deepest candidate with a child outside the chain.
        let mut deepest: Option<Arc<Mount>> = None;
        let mut current = Some(first.clone());
        while let Some(mount) = current {
            let next = self.next_candidate(&mount, victim);
            let outside = self
                .children_of(&mount)
                .iter()
                .any(|child| !is(child, &next) && !Arc::ptr_eq(child, victim));
            if outside {
                deepest = Some(mount.clone());
            }
            current = next;
        }
        let mut above = deepest.is_some();
        let mut current = Some(first.clone());
        while let Some(mount) = current {
            let next = self.next_candidate(&mount, victim);
            let over = self.overmount(&mount);
            let mut goes = !self
                .children_of(&mount)
                .iter()
                .any(|child| !is(child, &next) && !is(child, &over) && !Arc::ptr_eq(child, victim));
            let next_is_over = next.as_ref().is_some_and(|next| is(next, &over));
            let is_deepest = is(&mount, &deepest);
            if goes && next.is_some() && !next_is_over && above && !is_deepest {
                goes = false;
            }
            if is_deepest {
                above = false;
            }
            // Held here: `mount`, `deepest` if it is that one, and the caller's.
            let held = 1
                + usize::from(is_deepest)
                + held.iter().filter(|m| Arc::ptr_eq(m, &mount)).count();
            if goes && self.has_users(&mount, held) {
                return true;
            }
            current = next;
        }
        false
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

/// The state of one `propagate_umount`.
#[derive(Default)]
struct Umount {
    /// The mounts going: the set and those committed to going (`MNT_UMOUNT`).
    unmounted: HashSet<u64>,
    /// `T_UMOUNT_CANDIDATE`.
    candidate: HashSet<u64>,
    /// `T_MARKED`: `trim_ancestors` went up from here.
    marked: HashSet<u64>,
    /// The undecided candidates.
    list: VecDeque<Arc<Mount>>,
    /// The candidates committed to going.
    to_umount: Vec<Arc<Mount>>,
}

impl Umount {
    /// `remove_from_candidate_list`.
    fn remove_from_list(&mut self, mount: &Arc<Mount>) {
        self.candidate.remove(&mount.id);
        self.marked.remove(&mount.id);
        self.list.retain(|m| !Arc::ptr_eq(m, mount));
    }

    /// `umount_one`.
    fn umount_one(&mut self, mount: &Arc<Mount>) {
        if self.unmounted.insert(mount.id) {
            self.to_umount.push(mount.clone());
        }
    }
}
