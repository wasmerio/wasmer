//! Control groups, version 2 (`kernel/cgroup/cgroup.c`, `Documentation/admin-guide/cgroup-v2.rst`):
//! the one default hierarchy (`cgrp_dfl_root`), with the `memory` and `pids` controllers.
//!
//! A cgroup's mutable state lives in the hierarchy's [`Tree`], under one lock that is both
//! `cgroup_mutex` and `css_set_lock`; an [`Arc<Cgroup>`] is an immutable handle to it. A task's
//! place in the hierarchy (its `css_set`, with one hierarchy just its cgroup) is its [`Css`],
//! made when the task is (`cgroup_can_fork`, `cgroup_post_fork`), counted out of its cgroup's
//! population when the task exits (`cgroup_exit`) and released with the task
//! (`cgroup_release`), which is when the `pids` controller uncharges it. The task table is
//! locked before the tree, never after.
//!
//! The `pids` controller limits the tasks of a subtree at fork (`kernel/cgroup/pids.c`). The
//! `memory` controller shows what the subtree's address spaces hold; nothing is charged as
//! pages are touched, so its limits are kept and shown but not enforced. `cpu.stat` adds up the
//! CPU time the tasks were charged while they were in the subtree.

pub(crate) mod fs;
mod tasks;

pub(crate) use fs::CGROUP2_SUPER_MAGIC;

use std::{
    collections::{BTreeMap, HashMap},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use wasixv2_syscalls::{CpuAccount, CpuTimes, Errno};
use wasixv2_vfs::{Clock, Timestamp};

use crate::sync::MutexExt;

/// The controllers of the default hierarchy, one bit each, in Linux's order (`SUBSYS`):
/// `memory` before `pids`.
pub(crate) const MEMORY: u16 = 1 << 0;
pub(crate) const PIDS: u16 = 1 << 1;
const CONTROLLERS: [(u16, &str); 2] = [(MEMORY, "memory"), (PIDS, "pids")];
/// Every controller the hierarchy has (`root->subsys_mask`).
const ALL: u16 = MEMORY | PIDS;
/// The controllers that work inside a threaded subtree (`cgrp_dfl_threaded_ss_mask`).
const THREADED: u16 = PIDS;
/// `INT_MAX`: no limit on `cgroup.max.depth` and `cgroup.max.descendants`.
const UNLIMITED: u32 = i32::MAX as u32;
/// `PIDS_MAX`: one more than the largest pid (`PID_MAX_LIMIT`).
const PIDS_MAX: u64 = 4 * 1024 * 1024 + 1;

/// The mount options of the hierarchy (`cgrp_dfl_root.flags`), shared by every mount of it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RootFlags {
    pub(crate) nsdelegate: bool,
    pub(crate) favordynmods: bool,
    pub(crate) memory_localevents: bool,
    pub(crate) memory_recursiveprot: bool,
    pub(crate) memory_hugetlb_accounting: bool,
    pub(crate) pids_localevents: bool,
}

impl RootFlags {
    /// `cgroup2_parse_param`: sets the option `key`; `EINVAL` for one cgroup2 does not take.
    pub(crate) fn parse(&mut self, key: &[u8], value: Option<&[u8]>) -> Result<(), Errno> {
        if value.is_some() {
            return Err(Errno::EINVAL);
        }
        let flag = match key {
            b"nsdelegate" => &mut self.nsdelegate,
            b"favordynmods" => &mut self.favordynmods,
            b"memory_localevents" => &mut self.memory_localevents,
            b"memory_recursiveprot" => &mut self.memory_recursiveprot,
            b"memory_hugetlb_accounting" => &mut self.memory_hugetlb_accounting,
            b"pids_localevents" => &mut self.pids_localevents,
            _ => return Err(Errno::EINVAL),
        };
        *flag = true;
        Ok(())
    }

    /// `cgroup_show_options`.
    pub(crate) fn show(&self, out: &mut Vec<u8>) {
        let flags = [
            (self.nsdelegate, ",nsdelegate"),
            (self.favordynmods, ",favordynmods"),
            (self.memory_localevents, ",memory_localevents"),
            (self.memory_recursiveprot, ",memory_recursiveprot"),
            (self.memory_hugetlb_accounting, ",memory_hugetlb_accounting"),
            (self.pids_localevents, ",pids_localevents"),
        ];
        for (_, name) in flags.iter().filter(|(set, _)| *set) {
            out.extend_from_slice(name.as_bytes());
        }
    }
}

/// The default hierarchy.
pub(crate) struct Hierarchy {
    tree: Mutex<Tree>,
    root: Arc<Cgroup>,
    /// The next kernfs node id: a cgroup takes one for its directory and one per file it may
    /// have ([`fs::FILES`]).
    next_id: AtomicU64,
    /// Ids of cgroups whose last handle went, for the tree to forget on its next change.
    released: Released,
    /// Whether the hierarchy was ever mounted (`cgrp_dfl_visible`): only then does
    /// `/proc/<pid>/cgroup` show it.
    visible: AtomicBool,
    clock: Arc<dyn Clock>,
    /// The kernel log, which hears of refused forks.
    printk: Arc<crate::printk::Printk>,
}

/// A cgroup: its id (`cgroup_id`, the inode number of its directory), its parent and its
/// level (`cgrp->level`, 0 for the root).
pub(crate) struct Cgroup {
    pub(crate) id: u64,
    pub(crate) level: u32,
    pub(crate) parent: Option<Arc<Cgroup>>,
    released: Released,
}

/// Where a cgroup's handle records that it went. A handle does not reach the tree, so a
/// cgroup's type does not contain the whole hierarchy.
type Released = Arc<Mutex<Vec<u64>>>;

impl Drop for Cgroup {
    fn drop(&mut self) {
        self.released.lock_or_recover().push(self.id);
    }
}

impl Cgroup {
    /// Whether `self` is `ancestor` or below it (`cgroup_is_descendant`).
    pub(crate) fn descends_from(&self, ancestor: &Cgroup) -> bool {
        let mut at = Some(self);
        while let Some(cgroup) = at {
            if cgroup.id == ancestor.id {
                return true;
            }
            at = cgroup.parent.as_deref();
        }
        false
    }

    /// The ancestor of `self` at `level`.
    fn ancestor(self: &Arc<Self>, level: u32) -> Arc<Cgroup> {
        let mut at = self.clone();
        while at.level > level {
            at = at
                .parent
                .clone()
                .expect("a cgroup below the root has a parent");
        }
        at
    }

    /// The ancestors of `self` from its parent to the root.
    fn ancestors(&self) -> impl Iterator<Item = &Arc<Cgroup>> {
        std::iter::successors(self.parent.as_ref(), |cgroup| cgroup.parent.as_ref())
    }

    /// The nearest common ancestor of `a` and `b`.
    fn common(a: &Arc<Cgroup>, b: &Arc<Cgroup>) -> Arc<Cgroup> {
        let level = a.level.min(b.level);
        let (mut a, mut b) = (a.ancestor(level), b.ancestor(level));
        while a.id != b.id {
            a = a
                .parent
                .clone()
                .expect("cgroups of one hierarchy meet at its root");
            b = b
                .parent
                .clone()
                .expect("cgroups of one hierarchy meet at its root");
        }
        a
    }
}

/// The kind of a cgroup, as `cgroup.type` shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Domain,
    DomainThreaded,
    DomainInvalid,
    Threaded,
}

/// The ownership and times of a kernfs node: a cgroup's directory or one of its files.
#[derive(Clone, Copy, Debug)]
pub(crate) struct NodeAttr {
    pub(crate) mode: u32,
    pub(crate) uid: u32,
    pub(crate) gid: u32,
    pub(crate) atime: Timestamp,
    pub(crate) mtime: Timestamp,
    pub(crate) ctime: Timestamp,
}

/// The `pids` controller's state of a cgroup (`struct pids_cgroup`), while it has one.
#[derive(Clone, Copy, Debug, Default)]
struct PidsState {
    /// `pids.max`; `None` for `max`.
    limit: Option<u64>,
    /// `pids.peak` (`watermark`).
    watermark: u64,
    /// `pids.events` `max`: forks refused by this cgroup's limit or one below it.
    events_max: u64,
    /// `pids.events.local` `max`: forks refused by this cgroup's own limit.
    local_max: u64,
    /// Forks refused in this cgroup, whatever limit refused them (`PIDCG_FORKFAIL`).
    local_forkfail: u64,
}

/// The `memory` controller's settings of a cgroup (`struct mem_cgroup`), while it has one.
/// Nothing enforces them: there is no memory accounting.
#[derive(Clone, Copy, Debug, Default)]
struct MemoryState {
    min: u64,
    low: u64,
    /// `None` for `max`.
    high: Option<u64>,
    max: Option<u64>,
    oom_group: bool,
    /// `memory.peak`: the most the subtree held when it was looked at.
    peak: u64,
}

/// A cgroup's mutable state (`struct cgroup`).
struct State {
    name: Vec<u8>,
    /// The parent's id, for the counts kept above once the handle is gone.
    parent: Option<u64>,
    children: BTreeMap<Vec<u8>, Arc<Cgroup>>,
    /// Removed (`!CSS_ONLINE`): no task joins it, no child is made in it.
    dead: bool,
    subtree_control: u16,
    /// The domain of a threaded cgroup (`dom_cgrp`), its own id for a domain.
    dom: u64,
    nr_threaded_children: u32,
    nr_descendants: u32,
    nr_dying_descendants: u32,
    max_descendants: u32,
    max_depth: u32,
    /// Tasks in it that did not exit (`cgroup_has_tasks`).
    nr_tasks: u32,
    nr_populated_domain_children: u32,
    nr_populated_threaded_children: u32,
    /// `cgroup.freeze`: what was asked.
    freeze: bool,
    /// Tasks in its subtree from their fork until they are released (the `pids` counter).
    pids_count: u64,
    pids: PidsState,
    memory: MemoryState,
    /// CPU time of tasks that left it, while they were in it.
    cpu_retired: Cpu,
    /// The directory's ownership and times, and each file's, by its index in [`fs::FILES`].
    dir: NodeAttr,
    files: Vec<NodeAttr>,
}

impl State {
    fn populated(&self) -> bool {
        self.nr_tasks > 0
            || self.nr_populated_domain_children > 0
            || self.nr_populated_threaded_children > 0
    }

    fn threaded(&self, id: u64) -> bool {
        self.dom != id
    }
}

/// CPU time as `cpu.stat` adds it up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Cpu {
    pub(crate) times: CpuTimes,
    /// User time of tasks with a positive nice value (`CPUTIME_NICE`).
    pub(crate) nice: std::time::Duration,
}

impl Cpu {
    fn add(self, other: Cpu) -> Cpu {
        Cpu {
            times: self.times.saturating_add(other.times),
            nice: self.nice.saturating_add(other.nice),
        }
    }
}

/// Every cgroup's state, by id.
pub(crate) struct Tree {
    pub(crate) flags: RootFlags,
    nodes: HashMap<u64, State>,
    /// When the tree was locked: the time of whatever changes while it is.
    now: Timestamp,
}

impl Tree {
    fn node(&self, cgroup: &Cgroup) -> &State {
        &self.nodes[&cgroup.id]
    }

    fn node_mut(&mut self, cgroup: &Cgroup) -> &mut State {
        self.nodes
            .get_mut(&cgroup.id)
            .expect("a cgroup's state outlives its handles")
    }

    pub(crate) fn name(&self, cgroup: &Cgroup) -> Vec<u8> {
        self.node(cgroup).name.clone()
    }

    pub(crate) fn is_dead(&self, cgroup: &Cgroup) -> bool {
        self.node(cgroup).dead
    }

    pub(crate) fn children(&self, cgroup: &Cgroup) -> Vec<(Vec<u8>, Arc<Cgroup>)> {
        let node = self.node(cgroup);
        node.children
            .iter()
            .map(|(name, child)| (name.clone(), child.clone()))
            .collect()
    }

    pub(crate) fn child(&self, cgroup: &Cgroup, name: &[u8]) -> Option<Arc<Cgroup>> {
        self.node(cgroup).children.get(name).cloned()
    }

    pub(crate) fn dir_attr(&self, cgroup: &Cgroup) -> NodeAttr {
        self.node(cgroup).dir
    }

    pub(crate) fn dir_attr_mut(&mut self, cgroup: &Cgroup) -> &mut NodeAttr {
        &mut self.node_mut(cgroup).dir
    }

    pub(crate) fn file_attr(&self, cgroup: &Cgroup, file: usize) -> NodeAttr {
        self.node(cgroup).files[file]
    }

    pub(crate) fn file_attr_mut(&mut self, cgroup: &Cgroup, file: usize) -> &mut NodeAttr {
        &mut self.node_mut(cgroup).files[file]
    }

    pub(crate) fn nr_children(&self, cgroup: &Cgroup) -> usize {
        self.node(cgroup).children.len()
    }

    /// `cgroup_is_threaded`.
    pub(crate) fn is_threaded(&self, cgroup: &Cgroup) -> bool {
        self.node(cgroup).threaded(cgroup.id)
    }

    /// The domain of `cgroup` (`dom_cgrp`).
    fn dom(&self, cgroup: &Arc<Cgroup>) -> Arc<Cgroup> {
        let dom = self.node(cgroup).dom;
        std::iter::once(cgroup)
            .chain(cgroup.ancestors())
            .find(|c| c.id == dom)
            .expect("a threaded cgroup's domain is an ancestor")
            .clone()
    }

    /// `cgroup_is_mixable`: only the root.
    fn is_mixable(cgroup: &Cgroup) -> bool {
        cgroup.parent.is_none()
    }

    /// `cgroup_has_tasks`.
    fn has_tasks(&self, cgroup: &Cgroup) -> bool {
        self.node(cgroup).nr_tasks > 0
    }

    /// `cgroup_is_thread_root`.
    fn is_thread_root(&self, cgroup: &Cgroup) -> bool {
        let node = self.node(cgroup);
        if node.threaded(cgroup.id) {
            return false;
        }
        node.nr_threaded_children > 0 || (node.nr_tasks > 0 && node.subtree_control & THREADED != 0)
    }

    /// `cgroup_is_valid_domain`.
    fn is_valid_domain(&self, cgroup: &Cgroup) -> bool {
        if self.is_threaded(cgroup) {
            return false;
        }
        cgroup.ancestors().all(|ancestor| {
            (Self::is_mixable(ancestor) || !self.is_thread_root(ancestor))
                && !self.is_threaded(ancestor)
        })
    }

    /// `cgroup_can_be_thread_root`.
    fn can_be_thread_root(&self, cgroup: &Cgroup) -> bool {
        if Self::is_mixable(cgroup) {
            return true;
        }
        let node = self.node(cgroup);
        !node.threaded(cgroup.id)
            && node.nr_populated_domain_children == 0
            && node.subtree_control & !THREADED == 0
    }

    /// `cgroup.type`.
    pub(crate) fn kind(&self, cgroup: &Arc<Cgroup>) -> Kind {
        if self.is_threaded(cgroup) {
            Kind::Threaded
        } else if !self.is_valid_domain(cgroup) {
            Kind::DomainInvalid
        } else if self.is_thread_root(cgroup) {
            Kind::DomainThreaded
        } else {
            Kind::Domain
        }
    }

    /// `cgroup_control`: the controllers `cgroup` may enable for its children, which its
    /// `cgroup.controllers` lists: all of them in the root, else what its parent enabled, and
    /// only threaded ones in a threaded cgroup.
    pub(crate) fn control(&self, cgroup: &Cgroup) -> u16 {
        match &cgroup.parent {
            None => ALL,
            Some(parent) if self.is_threaded(cgroup) => {
                self.node(parent).subtree_control & THREADED
            }
            Some(parent) => self.node(parent).subtree_control,
        }
    }

    /// Whether `cgroup` has its own state of the controller `ss` (`cgroup_css`): the root
    /// has every controller's, a child those its parent enabled.
    pub(crate) fn has_css(&self, cgroup: &Cgroup, ss: u16) -> bool {
        cgroup.parent.is_none() || self.control(cgroup) & ss != 0
    }

    pub(crate) fn subtree_control(&self, cgroup: &Cgroup) -> u16 {
        self.node(cgroup).subtree_control
    }

    pub(crate) fn populated(&self, cgroup: &Cgroup) -> bool {
        self.node(cgroup).populated()
    }

    pub(crate) fn frozen(&self, cgroup: &Cgroup) -> bool {
        self.node(cgroup).freeze
    }

    /// `cgroup.max.depth` and `cgroup.max.descendants`.
    pub(crate) fn max_depth(&self, cgroup: &Cgroup) -> u32 {
        self.node(cgroup).max_depth
    }

    pub(crate) fn max_descendants(&self, cgroup: &Cgroup) -> u32 {
        self.node(cgroup).max_descendants
    }

    pub(crate) fn set_max_depth(&mut self, cgroup: &Cgroup, depth: u32) {
        self.node_mut(cgroup).max_depth = depth;
    }

    pub(crate) fn set_max_descendants(&mut self, cgroup: &Cgroup, descendants: u32) {
        self.node_mut(cgroup).max_descendants = descendants;
    }

    /// `cgroup_update_populated`: `cgroup` gained (`adj` 1) or lost (-1) a task that did not
    /// exit, which may make it, and so its ancestors, populated or empty.
    fn update_populated(&mut self, cgroup: &Cgroup, adj: i32) {
        let mut child: Option<&Cgroup> = None;
        let mut at = Some(cgroup);
        while let Some(cgroup) = at {
            let threaded_child = child.map(|child| self.is_threaded(child));
            let node = self.node_mut(cgroup);
            let was = node.populated();
            let count = match threaded_child {
                None => &mut node.nr_tasks,
                Some(true) => &mut node.nr_populated_threaded_children,
                Some(false) => &mut node.nr_populated_domain_children,
            };
            *count = count
                .checked_add_signed(adj)
                .expect("populated counts balance");
            let now = node.populated();
            if was == now {
                break;
            }
            let now = self.now;
            let node = self.node_mut(cgroup);
            // `cgroup_file_notify` of `cgroup.events`.
            let events = fs::file_index("cgroup.events");
            node.files[events].mtime = now;
            child = Some(cgroup);
            at = cgroup.parent.as_deref();
        }
    }

    /// `pids_charge` from `cgroup` up to, but not into, the root: `n` more tasks.
    fn pids_charge(&mut self, cgroup: &Cgroup, n: i64) {
        let mut at = Some(cgroup);
        while let Some(cgroup) = at.filter(|c| c.parent.is_some()) {
            let node = self.node_mut(cgroup);
            node.pids_count = node
                .pids_count
                .checked_add_signed(n)
                .expect("pids charges balance");
            node.pids.watermark = node.pids.watermark.max(node.pids_count);
            at = cgroup.parent.as_deref();
        }
    }

    /// The cgroup whose `pids` state a task in `cgroup` counts against (`task_css`): the
    /// nearest one, from `cgroup` up, that has the controller.
    fn pids_css<'a>(&self, cgroup: &'a Arc<Cgroup>) -> &'a Arc<Cgroup> {
        std::iter::once(cgroup)
            .chain(cgroup.ancestors())
            .find(|c| self.has_css(c, PIDS))
            .expect("the root has every controller")
    }

    /// `pids_can_fork`: charges a new task to `cgroup` unless that takes a cgroup with the
    /// controller, from its `pids` state up, past its `pids.max`, which counts as an event
    /// there and above (`pids_event`). The refusal names the cgroup whose fork it refused the
    /// first time one is.
    fn pids_try_charge(&mut self, cgroup: &Arc<Cgroup>) -> Result<(), Option<Arc<Cgroup>>> {
        let forking = self.pids_css(cgroup).clone();
        let over = std::iter::once(&forking)
            .chain(forking.ancestors())
            .filter(|c| c.parent.is_some() && self.has_css(c, PIDS))
            .find(|c| {
                let node = self.node(c);
                node.pids
                    .limit
                    .is_some_and(|limit| node.pids_count + 1 > limit)
            })
            .cloned();
        let Some(over) = over else {
            self.pids_charge(cgroup, 1);
            return Ok(());
        };
        let forkfail = &mut self.node_mut(&forking).pids.local_forkfail;
        *forkfail += 1;
        let first = *forkfail == 1;
        if !self.flags.pids_localevents {
            self.node_mut(&over).pids.local_max += 1;
            let mut at = Some(&over);
            while let Some(cgroup) = at.filter(|c| c.parent.is_some()) {
                self.node_mut(cgroup).pids.events_max += 1;
                at = cgroup.parent.as_ref();
            }
        }
        Err(first.then_some(forking))
    }

    /// `pids.current`, `pids.peak`, `pids.max` and the two event files' counts.
    pub(crate) fn pids_current(&self, cgroup: &Cgroup) -> u64 {
        self.node(cgroup).pids_count
    }

    pub(crate) fn pids_peak(&self, cgroup: &Cgroup) -> u64 {
        self.node(cgroup).pids.watermark
    }

    pub(crate) fn pids_max(&self, cgroup: &Cgroup) -> Option<u64> {
        self.node(cgroup).pids.limit
    }

    /// `pids_max_write`: a limit below `PIDS_MAX`, or none.
    pub(crate) fn set_pids_max(&mut self, cgroup: &Cgroup, limit: Option<u64>) {
        self.node_mut(cgroup).pids.limit = limit;
    }

    /// `__pids_events_show`: the hierarchical or local `max` count, or with `pids_localevents`
    /// the local count of refused forks.
    pub(crate) fn pids_events(&self, cgroup: &Cgroup, local: bool) -> u64 {
        let pids = &self.node(cgroup).pids;
        match (self.flags.pids_localevents, local) {
            (true, _) => pids.local_forkfail,
            (false, true) => pids.local_max,
            (false, false) => pids.events_max,
        }
    }

    /// The `memory` settings.
    pub(crate) fn memory(&self, cgroup: &Cgroup) -> MemoryLimits {
        let memory = self.node(cgroup).memory;
        MemoryLimits {
            min: memory.min,
            low: memory.low,
            high: memory.high,
            max: memory.max,
            oom_group: memory.oom_group,
        }
    }

    pub(crate) fn set_memory(&mut self, cgroup: &Cgroup, set: impl FnOnce(&mut MemoryLimits)) {
        let mut limits = self.memory(cgroup);
        set(&mut limits);
        let memory = &mut self.node_mut(cgroup).memory;
        memory.min = limits.min;
        memory.low = limits.low;
        memory.high = limits.high;
        memory.max = limits.max;
        memory.oom_group = limits.oom_group;
    }

    /// `memory.peak` after the subtree was seen holding `current` bytes.
    pub(crate) fn memory_peak(&mut self, cgroup: &Cgroup, current: u64) -> u64 {
        let memory = &mut self.node_mut(cgroup).memory;
        memory.peak = memory.peak.max(current);
        memory.peak
    }

    /// `cgroup.stat`: live descendants, dying ones, and per controller how many cgroups of
    /// the subtree have its state.
    pub(crate) fn stat(&self, cgroup: &Arc<Cgroup>) -> Stat {
        let node = self.node(cgroup);
        let mut subsys = [0u32; CONTROLLERS.len()];
        let mut stack = vec![cgroup.clone()];
        while let Some(at) = stack.pop() {
            for (i, (ss, _)) in CONTROLLERS.iter().enumerate() {
                if self.has_css(&at, *ss) {
                    subsys[i] += 1;
                }
            }
            stack.extend(self.node(&at).children.values().cloned());
        }
        Stat {
            nr_descendants: node.nr_descendants,
            nr_dying_descendants: node.nr_dying_descendants,
            subsys,
        }
    }

    /// The CPU time retired into `cgroup` and every cgroup below it.
    pub(crate) fn cpu_retired(&self, cgroup: &Arc<Cgroup>) -> Cpu {
        let mut total = Cpu::default();
        let mut stack = vec![cgroup.clone()];
        while let Some(at) = stack.pop() {
            let node = self.node(&at);
            total = total.add(node.cpu_retired);
            stack.extend(node.children.values().cloned());
        }
        total
    }

    /// The live cgroups of the hierarchy, for `/proc/cgroups` (`root->nr_cgrps`).
    pub(crate) fn nr_cgroups(&self) -> usize {
        self.nodes.values().filter(|node| !node.dead).count()
    }

    /// `cgroup_vet_subtree_control_enable` and `cgroup_subtree_control_write`: enables and
    /// disables controllers for `cgroup`'s children. `ENOENT` for one `cgroup` does not have;
    /// `EBUSY` to disable one a child enabled further down, or to enable a domain controller
    /// while `cgroup` holds tasks; `EOPNOTSUPP` for a domain controller in a threaded subtree.
    pub(crate) fn set_subtree_control(
        &mut self,
        cgroup: &Arc<Cgroup>,
        mut enable: u16,
        mut disable: u16,
    ) -> Result<(), Errno> {
        let node = self.node(cgroup);
        let control = self.control(cgroup);
        for (ss, _) in CONTROLLERS {
            if enable & ss != 0 {
                if node.subtree_control & ss != 0 {
                    enable &= !ss;
                } else if control & ss == 0 {
                    return Err(Errno::ENOENT);
                }
            } else if disable & ss != 0 {
                if node.subtree_control & ss == 0 {
                    disable &= !ss;
                } else if node
                    .children
                    .values()
                    .any(|child| !self.is_dead(child) && self.subtree_control(child) & ss != 0)
                {
                    return Err(Errno::EBUSY);
                }
            }
        }
        if enable == 0 && disable == 0 {
            return Ok(());
        }
        self.vet_subtree_control_enable(cgroup, enable)?;
        let children: Vec<Arc<Cgroup>> = node.children.values().cloned().collect();
        self.node_mut(cgroup).subtree_control = (node_control(self, cgroup) | enable) & !disable;
        // A child's controller state goes with the controller (`kill_css`), and starts afresh
        // when it is enabled again.
        for child in children {
            let node = self.node_mut(&child);
            if disable & PIDS != 0 {
                node.pids = PidsState::default();
            }
            if disable & MEMORY != 0 {
                node.memory = MemoryState::default();
            }
        }
        Ok(())
    }

    fn vet_subtree_control_enable(&self, cgroup: &Arc<Cgroup>, enable: u16) -> Result<(), Errno> {
        if enable == 0 {
            return Ok(());
        }
        let dom = self.dom(cgroup);
        if !self.is_valid_domain(&dom) {
            return Err(Errno::EOPNOTSUPP);
        }
        if Self::is_mixable(cgroup) {
            return Ok(());
        }
        if enable & !THREADED != 0 {
            if self.is_thread_root(cgroup) || self.is_threaded(cgroup) {
                return Err(Errno::EOPNOTSUPP);
            }
        } else if self.can_be_thread_root(cgroup) || self.is_threaded(cgroup) {
            return Ok(());
        }
        if self.has_tasks(cgroup) {
            return Err(Errno::EBUSY);
        }
        Ok(())
    }

    /// `cgroup_enable_threaded`: `cgroup` joins its parent's domain.
    pub(crate) fn enable_threaded(&mut self, cgroup: &Arc<Cgroup>) -> Result<(), Errno> {
        if self.is_threaded(cgroup) {
            return Ok(());
        }
        let parent = cgroup.parent.as_ref().expect("the root has no cgroup.type");
        let dom = self.dom(parent);
        if self.populated(cgroup) || self.subtree_control(cgroup) & !THREADED != 0 {
            return Err(Errno::EOPNOTSUPP);
        }
        if !self.is_valid_domain(&dom) || !self.can_be_thread_root(&dom) {
            return Err(Errno::EOPNOTSUPP);
        }
        let mut stack = vec![cgroup.clone()];
        while let Some(at) = stack.pop() {
            if at.id == cgroup.id || self.is_threaded(&at) {
                self.node_mut(&at).dom = dom.id;
            }
            stack.extend(self.node(&at).children.values().cloned());
        }
        self.node_mut(parent).nr_threaded_children += 1;
        Ok(())
    }

    /// `cgroup_migrate_vet_dst`: whether tasks may move into `dst`: `EOPNOTSUPP` for a cgroup
    /// that cannot hold them, `EBUSY` for one whose children have domain controllers (the
    /// no-internal-process rule).
    pub(crate) fn vet_dst(&self, dst: &Arc<Cgroup>) -> Result<(), Errno> {
        let dom = self.dom(dst);
        if !self.is_valid_domain(&dom) {
            return Err(Errno::EOPNOTSUPP);
        }
        if self.can_be_thread_root(dst) || self.is_threaded(dst) {
            return Ok(());
        }
        if self.subtree_control(dst) != 0 {
            return Err(Errno::EBUSY);
        }
        Ok(())
    }

    /// `cgroup_attach_permissions` past the permission checks: `dst` must take tasks, and a
    /// single thread (`threadgroup` false) stays in its domain (`EOPNOTSUPP`).
    pub(crate) fn may_attach(
        &self,
        src: &Arc<Cgroup>,
        dst: &Arc<Cgroup>,
        threadgroup: bool,
    ) -> Result<(), Errno> {
        if self.is_dead(dst) {
            return Err(Errno::ENODEV);
        }
        self.vet_dst(dst)?;
        if !threadgroup && self.node(src).dom != self.node(dst).dom {
            return Err(Errno::EOPNOTSUPP);
        }
        Ok(())
    }

    /// `cgroup_check_hierarchy_limits` then `cgroup_create`: a child `name` of `parent`,
    /// owned by `owner` with the directory mode `mode`. `ENODEV` in a removed cgroup,
    /// `EAGAIN` past a `cgroup.max.depth` or `cgroup.max.descendants` above, `EEXIST` for a
    /// name in use, by a child or a file.
    fn create(
        &mut self,
        hierarchy: &Arc<Hierarchy>,
        parent: &Arc<Cgroup>,
        name: &[u8],
        mode: u32,
        owner: (u32, u32),
    ) -> Result<Arc<Cgroup>, Errno> {
        if name.contains(&b'\n') {
            return Err(Errno::EINVAL);
        }
        if self.is_dead(parent) {
            return Err(Errno::ENODEV);
        }
        for (level, cgroup) in std::iter::once(parent)
            .chain(parent.ancestors())
            .enumerate()
        {
            let node = self.node(cgroup);
            if node.nr_descendants >= node.max_descendants || level as u32 >= node.max_depth {
                return Err(Errno::EAGAIN);
            }
        }
        if self.node(parent).children.contains_key(name) || fs::file_index_of(name).is_some() {
            return Err(Errno::EEXIST);
        }
        let id = hierarchy.alloc_id();
        let cgroup = Arc::new(Cgroup {
            id,
            level: parent.level + 1,
            parent: Some(parent.clone()),
            released: hierarchy.released.clone(),
        });
        let parent_node = self.node(parent);
        // `cgroup_create`: a child of a threaded cgroup is threaded, in its domain.
        let dom = match parent_node.threaded(parent.id) {
            true => parent_node.dom,
            false => id,
        };
        let now = self.now;
        self.nodes
            .insert(id, State::new(name, Some(parent.id), dom, mode, owner, now));
        if dom != id {
            self.node_mut(parent).nr_threaded_children += 1;
        }
        for ancestor in std::iter::once(parent).chain(parent.ancestors()) {
            self.node_mut(ancestor).nr_descendants += 1;
        }
        let parent_node = self.node_mut(parent);
        parent_node.children.insert(name.to_vec(), cgroup.clone());
        parent_node.dir.mtime = now;
        parent_node.dir.ctime = now;
        Ok(cgroup)
    }

    /// `cgroup_destroy_locked`: removes `cgroup`, which must hold no task that did not exit
    /// and have no children (`EBUSY`). Its exited tasks keep it, dead, until they are
    /// released.
    fn destroy(&mut self, cgroup: &Arc<Cgroup>) -> Result<(), Errno> {
        let node = self.node(cgroup);
        if node.dead {
            return Err(Errno::ENOENT);
        }
        if node.nr_tasks > 0 || node.children.values().any(|c| !self.is_dead(c)) {
            return Err(Errno::EBUSY);
        }
        let threaded = node.threaded(cgroup.id);
        let name = node.name.clone();
        let now = self.now;
        self.node_mut(cgroup).dead = true;
        let parent = cgroup.parent.as_ref().expect("the root is never removed");
        if threaded {
            self.node_mut(parent).nr_threaded_children -= 1;
        }
        for ancestor in cgroup.ancestors() {
            let node = self.node_mut(ancestor);
            node.nr_descendants -= 1;
            node.nr_dying_descendants += 1;
        }
        let parent_node = self.node_mut(parent);
        // The handle may be the last; its state goes on the tree's next lock.
        parent_node.children.remove(&name);
        parent_node.dir.mtime = now;
        parent_node.dir.ctime = now;
        Ok(())
    }

    /// The last handle of the cgroup `id` went: a removed cgroup is no longer dying
    /// (`css_release`), and its state goes.
    fn release(&mut self, id: u64) {
        let Some(node) = self.nodes.remove(&id) else {
            return;
        };
        let mut at = node.parent.filter(|_| node.dead);
        while let Some(id) = at {
            let ancestor = self
                .nodes
                .get_mut(&id)
                .expect("a parent outlives its children");
            ancestor.nr_dying_descendants -= 1;
            at = ancestor.parent;
        }
    }

    /// `cgroup_freeze`'s request, recorded.
    pub(crate) fn set_freeze(&mut self, cgroup: &Cgroup, freeze: bool) {
        self.node_mut(cgroup).freeze = freeze;
    }
}

/// The subtree control of `cgroup` (a helper to read it before a mutable borrow).
fn node_control(tree: &Tree, cgroup: &Cgroup) -> u16 {
    tree.node(cgroup).subtree_control
}

impl State {
    fn new(
        name: &[u8],
        parent: Option<u64>,
        dom: u64,
        mode: u32,
        (uid, gid): (u32, u32),
        now: Timestamp,
    ) -> Self {
        let attr = |mode| NodeAttr {
            mode,
            uid,
            gid,
            atime: now,
            mtime: now,
            ctime: now,
        };
        Self {
            name: name.to_vec(),
            parent,
            children: BTreeMap::new(),
            dead: false,
            subtree_control: 0,
            dom,
            nr_threaded_children: 0,
            nr_descendants: 0,
            nr_dying_descendants: 0,
            max_descendants: UNLIMITED,
            max_depth: UNLIMITED,
            nr_tasks: 0,
            nr_populated_domain_children: 0,
            nr_populated_threaded_children: 0,
            freeze: false,
            pids_count: 0,
            pids: PidsState::default(),
            memory: MemoryState::default(),
            cpu_retired: Cpu::default(),
            dir: attr(mode & 0o7777),
            files: fs::FILES.iter().map(|file| attr(file.mode())).collect(),
        }
    }
}

/// The `memory` controller's settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct MemoryLimits {
    pub(crate) min: u64,
    pub(crate) low: u64,
    pub(crate) high: Option<u64>,
    pub(crate) max: Option<u64>,
    pub(crate) oom_group: bool,
}

/// What `cgroup.stat` shows.
pub(crate) struct Stat {
    pub(crate) nr_descendants: u32,
    pub(crate) nr_dying_descendants: u32,
    /// Per controller, in [`CONTROLLERS`]' order.
    pub(crate) subsys: [u32; CONTROLLERS.len()],
}

/// The names of the controllers in `mask`, space-separated (`cgroup_print_ss_mask`).
pub(crate) fn mask_names(mask: u16) -> String {
    let names: Vec<&str> = CONTROLLERS
        .iter()
        .filter(|(ss, _)| mask & ss != 0)
        .map(|(_, name)| *name)
        .collect();
    names.join(" ")
}

/// The controller named `name`.
pub(crate) fn controller(name: &[u8]) -> Option<u16> {
    CONTROLLERS
        .iter()
        .find(|(_, known)| known.as_bytes() == name)
        .map(|(ss, _)| *ss)
}

/// The controller names, for `cgroup.stat` and `/proc/cgroups`.
pub(crate) fn controller_names() -> impl Iterator<Item = &'static str> {
    CONTROLLERS.iter().map(|(_, name)| *name)
}

impl Hierarchy {
    /// The hierarchy with nothing but its root, owned by root (`cgroup_setup_root`).
    pub(crate) fn new(clock: Arc<dyn Clock>, printk: Arc<crate::printk::Printk>) -> Arc<Self> {
        let now = clock.now();
        let released = Released::default();
        let root = Arc::new(Cgroup {
            id: 1,
            level: 0,
            parent: None,
            released: released.clone(),
        });
        let mut nodes = HashMap::new();
        nodes.insert(1, State::new(b"", None, 1, 0o755, (0, 0), now));
        Arc::new(Self {
            tree: Mutex::new(Tree {
                flags: RootFlags::default(),
                nodes,
                now,
            }),
            root,
            next_id: AtomicU64::new(2 + fs::FILES.len() as u64),
            released,
            visible: AtomicBool::new(false),
            clock,
            printk,
        })
    }

    pub(crate) fn root(&self) -> &Arc<Cgroup> {
        &self.root
    }

    /// Locks the tree (`cgroup_lock`), forgetting the cgroups whose handles all went. A
    /// handle may go while the tree is locked: it only records itself in `released`, a lock
    /// this one never holds while it waits for the tree.
    pub(crate) fn lock(&self) -> MutexGuard<'_, Tree> {
        let released = std::mem::take(&mut *self.released.lock_or_recover());
        let mut tree = self.tree.lock_or_recover();
        for id in released {
            tree.release(id);
        }
        tree.now = self.clock.now();
        tree
    }

    fn alloc_id(&self) -> u64 {
        self.next_id
            .fetch_add(1 + fs::FILES.len() as u64, Ordering::Relaxed)
    }

    /// The kernfs id of file `file` of `cgroup`.
    pub(crate) fn file_ino(cgroup: &Cgroup, file: usize) -> u64 {
        cgroup.id + 1 + file as u64
    }

    pub(crate) fn mark_visible(&self) {
        self.visible.store(true, Ordering::Relaxed);
    }

    pub(crate) fn is_visible(&self) -> bool {
        self.visible.load(Ordering::Relaxed)
    }

    /// `cgroup_mkdir`.
    pub(crate) fn mkdir(
        self: &Arc<Self>,
        parent: &Arc<Cgroup>,
        name: &[u8],
        mode: u32,
        owner: (u32, u32),
    ) -> Result<Arc<Cgroup>, Errno> {
        self.lock().create(self, parent, name, mode, owner)
    }

    /// `cgroup_rmdir`.
    pub(crate) fn rmdir(&self, parent: &Cgroup, name: &[u8]) -> Result<(), Errno> {
        let mut tree = self.lock();
        let cgroup = tree.child(parent, name).ok_or(Errno::ENOENT)?;
        tree.destroy(&cgroup)
    }

    /// The path of `cgroup` as seen from `root`, a cgroup namespace's root
    /// (`cgroup_path_ns`, `kernfs_path_from_node`): `/` and the names below `root`, with `..`
    /// for each level `cgroup` is above it.
    pub(crate) fn path_from(&self, cgroup: &Arc<Cgroup>, root: &Arc<Cgroup>) -> Vec<u8> {
        let tree = self.lock();
        let common = Cgroup::common(cgroup, root);
        let mut out = Vec::new();
        for _ in common.level..root.level {
            out.extend_from_slice(b"/..");
        }
        let mut names = Vec::new();
        let mut at = cgroup.clone();
        while at.level > common.level {
            names.push(tree.name(&at));
            at = at.parent.clone().expect("below the common ancestor");
        }
        for name in names.iter().rev() {
            out.push(b'/');
            out.extend_from_slice(name);
        }
        if out.is_empty() {
            out.push(b'/');
        }
        out
    }

    /// `proc_cgroup_show`'s line for a task in `cgroup`, as a reader whose cgroup namespace
    /// is rooted at `root` sees it; none while the hierarchy was never mounted.
    pub(crate) fn proc_line(&self, cgroup: &Arc<Cgroup>, root: &Arc<Cgroup>) -> Option<Vec<u8>> {
        if !self.is_visible() {
            return None;
        }
        let mut line = b"0::".to_vec();
        line.extend(self.path_from(cgroup, root));
        if self.lock().is_dead(cgroup) {
            line.extend_from_slice(b" (deleted)");
        }
        line.push(b'\n');
        Some(line)
    }

    /// `cgroup_can_fork` and `cgroup_post_fork` for a task made in `cgroup`: charged to
    /// `pids`, counted in the population. `ENODEV` for a removed cgroup (`CLONE_INTO_CGROUP`),
    /// `EAGAIN` past a `pids.max`.
    pub(crate) fn fork(
        self: &Arc<Self>,
        cgroup: Arc<Cgroup>,
        cpu: Arc<CpuAccount>,
        nice: bool,
    ) -> Result<Css, Errno> {
        let mut tree = self.lock();
        if tree.is_dead(&cgroup) {
            return Err(Errno::ENODEV);
        }
        if let Err(first) = tree.pids_try_charge(&cgroup) {
            drop(tree);
            // `pids_event`: said once per cgroup.
            if let Some(forking) = first {
                let path =
                    String::from_utf8_lossy(&self.path_from(&forking, &self.root)).into_owned();
                let text = format!("cgroup: fork rejected by pids controller in {path}");
                self.printk.log(crate::printk::Level::Info, &text);
            }
            return Err(Errno::EAGAIN);
        }
        tree.update_populated(&cgroup, 1);
        drop(tree);
        Ok(Css::new(CssState {
            base: cpu.times(),
            cpu,
            cgroup,
            hierarchy: self.clone(),
            exited: false,
            nice,
        }))
    }
}

/// A task's place in the hierarchy (`task->cgroups`): see the module's documentation. A
/// zombie's process keeps its leader's until it is reaped, as the leader's `task_struct`
/// keeps it on Linux; the last holder releases it. Locked after the tree, never before.
#[derive(Clone)]
pub(crate) struct Css {
    hierarchy: Arc<Hierarchy>,
    inner: Arc<Mutex<CssState>>,
}

struct CssState {
    cgroup: Arc<Cgroup>,
    hierarchy: Arc<Hierarchy>,
    /// The task's CPU account, and what it held when the task joined `cgroup`: the cgroup
    /// counts only what came after.
    cpu: Arc<CpuAccount>,
    base: CpuTimes,
    /// Whether the task's user time counts as niced (`task_nice > 0`) when retired.
    nice: bool,
    /// `cgroup_exit` ran: the task no longer populates its cgroup.
    exited: bool,
}

impl CssState {
    /// The task's CPU time in its cgroup so far.
    fn cpu(&self) -> Cpu {
        let times = self.cpu.times();
        let used = CpuTimes {
            user: times.user.saturating_sub(self.base.user),
            system: times.system.saturating_sub(self.base.system),
        };
        Cpu {
            times: used,
            nice: match self.nice {
                true => used.user,
                false => Default::default(),
            },
        }
    }

    /// Adds the task's CPU time in its cgroup to the cgroup's, and counts from now.
    fn retire(&mut self, tree: &mut Tree) {
        let used = self.cpu();
        let node = tree.node_mut(&self.cgroup);
        node.cpu_retired = node.cpu_retired.add(used);
        self.base = self.cpu.times();
    }
}

impl Css {
    fn new(state: CssState) -> Self {
        Self {
            hierarchy: state.hierarchy.clone(),
            inner: Arc::new(Mutex::new(state)),
        }
    }

    fn state(&self) -> MutexGuard<'_, CssState> {
        self.inner.lock_or_recover()
    }

    pub(crate) fn cgroup(&self) -> Arc<Cgroup> {
        self.state().cgroup.clone()
    }

    pub(crate) fn exited(&self) -> bool {
        self.state().exited
    }

    /// The task's CPU time in its cgroup so far.
    pub(crate) fn cpu(&self) -> Cpu {
        self.state().cpu()
    }

    /// Records whether the task runs niced: its time so far counts as it was, what comes
    /// after as `nice` says.
    pub(crate) fn set_nice(&self, nice: bool) {
        let mut tree = self.hierarchy.lock();
        let mut state = self.state();
        if state.nice != nice {
            state.retire(&mut tree);
            state.nice = nice;
        }
    }

    /// `cgroup_exit`: the task no longer populates its cgroup, which keeps it until it is
    /// released, and the time it ran counts there (`cgroup_rstat_updated`). Once.
    pub(crate) fn exit(&self) {
        let mut tree = self.hierarchy.lock();
        let mut state = self.state();
        if !std::mem::replace(&mut state.exited, true) {
            tree.update_populated(&state.cgroup, -1);
            state.retire(&mut tree);
        }
    }

    /// Moves the task to `dst` (`css_set_move_task` with the `pids` controller's
    /// `pids_can_attach`, which charges the destination whatever its limit).
    pub(crate) fn migrate(&self, tree: &mut Tree, dst: &Arc<Cgroup>) {
        let mut state = self.state();
        if state.cgroup.id == dst.id {
            return;
        }
        if !state.exited {
            tree.update_populated(&state.cgroup, -1);
            tree.update_populated(dst, 1);
        }
        tree.pids_charge(dst, 1);
        tree.pids_charge(&state.cgroup, -1);
        state.retire(tree);
        state.cgroup = dst.clone();
    }

    /// Exchanges the cgroups of two tasks, as `de_thread` exchanges the pids of the task that
    /// calls `execve` and its leader: the survivor keeps its own place.
    pub(crate) fn exchange(a: &Css, b: &Css) {
        let mut tree = a.hierarchy.lock();
        let (mut a, mut b) = (a.state(), b.state());
        a.retire(&mut tree);
        b.retire(&mut tree);
        std::mem::swap(&mut a.cgroup, &mut b.cgroup);
        std::mem::swap(&mut a.exited, &mut b.exited);
    }
}

impl Drop for CssState {
    /// `cgroup_release` with the `pids` controller's `pids_release`.
    fn drop(&mut self) {
        let hierarchy = self.hierarchy.clone();
        let mut tree = hierarchy.lock();
        if !self.exited {
            tree.update_populated(&self.cgroup, -1);
        }
        tree.pids_charge(&self.cgroup, -1);
        self.retire(&mut tree);
    }
}

impl crate::Kernel {
    /// The cgroup whose directory the descriptor `fd` names, for `CLONE_INTO_CGROUP`
    /// (`fdget_raw`, `cgroup_get_from_file`): `EBADF` for no descriptor or one that is not a
    /// `cgroup2` directory.
    pub(crate) fn cgroup_of_fd(
        &self,
        caller: &wasixv2_syscalls::Caller<'_>,
        fd: u32,
    ) -> Result<Arc<Cgroup>, Errno> {
        let fd = wasixv2_syscalls::Fd(i32::try_from(fd).map_err(|_| Errno::EBADF)?);
        let file = self.file(caller.task(), fd)?;
        let path = file.path();
        if path.dentry.sb().info.magic != CGROUP2_SUPER_MAGIC {
            return Err(Errno::EBADF);
        }
        fs::cgroup_of(&**path.dentry.node()).ok_or(Errno::EBADF)
    }

    /// `cgroup_can_fork` for a child of `parent` (whose credentials are `cred`): in its
    /// parent's cgroup, or in `into` (`cgroup_css_set_fork`), which must be live (`ENODEV`)
    /// and one the parent may move a process to from its own; charged to `pids`.
    pub(crate) fn cgroup_can_fork(
        &self,
        parent: &crate::Task,
        cred: &crate::cred::Cred,
        into: Option<Result<Arc<Cgroup>, Errno>>,
        cpu: Arc<CpuAccount>,
        nice: bool,
    ) -> Result<Css, Errno> {
        let src = parent.css.cgroup();
        let Some(dst) = into else {
            return self.cgroups.fork(src, cpu, nice);
        };
        let dst = dst?;
        if self.cgroups.lock().is_dead(&dst) {
            return Err(Errno::ENODEV);
        }
        let ns_root = &parent.live_ns().cgroup.root;
        let common = Cgroup::common(&src, &dst);
        tasks::may_write_procs(&self.cgroups, &common, Some(&cred.fs()))?;
        {
            let tree = self.cgroups.lock();
            if tree.flags.nsdelegate && (!src.descends_from(ns_root) || !dst.descends_from(ns_root))
            {
                return Err(Errno::ENOENT);
            }
            tree.may_attach(&src, &dst, true)?;
        }
        self.cgroups.fork(dst, cpu, nice)
    }

    /// A `cgroup2` instance for a mount from the cgroup namespace `ns` (`cgroup_init_fs_context`,
    /// `cgroup_get_tree`): rooted at the namespace's root, and with `flags`, the mount's
    /// options, applied to the hierarchy.
    pub(crate) fn cgroup_fs(
        &self,
        ns: &crate::ns::CgroupNs,
        flags: RootFlags,
    ) -> Arc<dyn wasixv2_vfs::Filesystem> {
        self.apply_cgroup_root_flags(ns, flags);
        let state = Arc::downgrade(&self.state);
        Arc::new(fs::CgroupFs::new(
            self.cgroups.clone(),
            ns.root.clone(),
            state,
        ))
    }

    /// `apply_cgroup_root_flags`: the hierarchy's options become `flags` when they come from
    /// the initial cgroup namespace; from any other they change nothing.
    pub(crate) fn apply_cgroup_root_flags(&self, ns: &crate::ns::CgroupNs, flags: RootFlags) {
        if std::ptr::eq(ns, &*self.init_ns.cgroup) {
            self.cgroups.lock().flags = flags;
        }
    }

    /// The `cgroup2` filesystem of the initial cgroup namespace, to mount at
    /// `/sys/fs/cgroup`.
    pub fn cgroupfs(&self) -> Arc<dyn wasixv2_vfs::Filesystem> {
        let state = Arc::downgrade(&self.state);
        let root = self.cgroups.root().clone();
        Arc::new(fs::CgroupFs::new(self.cgroups.clone(), root, state))
    }
}
