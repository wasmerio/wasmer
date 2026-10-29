//! Classic BPF socket filters (`net/core/filter.c`): `SO_ATTACH_FILTER` checks a program as
//! `bpf_check_classic` does, and a socket runs it over what it receives before queueing it
//! (`sk_filter_trim_cap`): 0 drops the packet, a smaller length cuts it short. There is no
//! eBPF (`SO_ATTACH_BPF`).
//!
//! The same programs filter syscalls (`kernel/seccomp.c`): [`Program::seccomp`] checks one as
//! `seccomp_check_filter` does, and it runs over the [`Input`] that is a `seccomp_data`.
//!
//! There are still no packets: a socket builds what its `sk_buff` would hold ([`Packet`]),
//! the IP header and transport header in front of the data, and a link-layer header made up
//! from the receiving interface (its address as the destination, a group's for multicast, no
//! source, as there are no neighbour tables).

use std::{
    collections::hash_map::RandomState,
    hash::BuildHasher,
    net::IpAddr,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use wasixv2_abi::{Errno, GuestLayout, SockFilter as Insn, SocketOption as So};

use crate::{
    NetResult,
    icmp::{IPV6_HDR, ipv4_header, ipv6_header, v6},
    iface::LinkKind,
    net::Net,
    opts::{read_int, write_int},
};

/// `BPF_MAXINSNS`, `BPF_MEMWORDS`.
const MAXINSNS: usize = 4096;
const MEMWORDS: u32 = 16;
/// `SO_GET_FILTER` is `SO_ATTACH_FILTER`; `SO_BPF_EXTENSIONS` answers `SKF_AD_MAX`.
pub const SO_GET_FILTER: i32 = So::SO_ATTACH_FILTER.0;
const SKF_AD_MAX: i32 = 64;
/// `sizeof(struct sock_filter)`.
pub const INSN_LEN: usize = Insn::SIZE;
/// The longest program, in bytes.
pub const MAX_PROGRAM_LEN: usize = MAXINSNS * INSN_LEN;

// Instruction classes, sizes, modes and operations (`include/uapi/linux/bpf_common.h`).
const LD: u16 = 0x00;
const LDX: u16 = 0x01;
const ST: u16 = 0x02;
const STX: u16 = 0x03;
const ALU: u16 = 0x04;
const JMP: u16 = 0x05;
const RET: u16 = 0x06;
const MISC: u16 = 0x07;
const W: u16 = 0x00;
const H: u16 = 0x08;
const B: u16 = 0x10;
const IMM: u16 = 0x00;
const ABS: u16 = 0x20;
const IND: u16 = 0x40;
const MEM: u16 = 0x60;
const LEN: u16 = 0x80;
const MSH: u16 = 0xa0;
const ADD: u16 = 0x00;
const SUB: u16 = 0x10;
const MUL: u16 = 0x20;
const DIV: u16 = 0x30;
const OR: u16 = 0x40;
const AND: u16 = 0x50;
const LSH: u16 = 0x60;
const RSH: u16 = 0x70;
const NEG: u16 = 0x80;
const MOD: u16 = 0x90;
const XOR: u16 = 0xa0;
const JA: u16 = 0x00;
const JEQ: u16 = 0x10;
const JGT: u16 = 0x20;
const JGE: u16 = 0x30;
const JSET: u16 = 0x40;
const K: u16 = 0x00;
const X: u16 = 0x08;
const RVAL_A: u16 = 0x10;
const TAX: u16 = 0x00;
const TXA: u16 = 0x80;

/// `SKF_AD_OFF`, `SKF_NET_OFF`, `SKF_LL_OFF` and the ancillary loads `bpf_anc_helper` knows.
const SKF_AD_OFF: i32 = -0x1000;
const SKF_NET_OFF: i32 = -0x100000;
const SKF_LL_OFF: i32 = -0x200000;
const AD_PROTOCOL: i32 = 0;
const AD_PKTTYPE: i32 = 4;
const AD_IFINDEX: i32 = 8;
const AD_NLATTR: i32 = 12;
const AD_NLATTR_NEST: i32 = 16;
const AD_MARK: i32 = 20;
const AD_QUEUE: i32 = 24;
const AD_HATYPE: i32 = 28;
const AD_RXHASH: i32 = 32;
const AD_CPU: i32 = 36;
const AD_ALU_XOR_X: i32 = 40;
const AD_VLAN_TAG: i32 = 44;
const AD_VLAN_TAG_PRESENT: i32 = 48;
const AD_PAY_OFFSET: i32 = 52;
const AD_RANDOM: i32 = 56;
const AD_VLAN_TPID: i32 = 60;

/// `PACKET_HOST`, `PACKET_MULTICAST`, `PACKET_LOOPBACK` (`skb->pkt_type`).
pub const PACKET_HOST: u8 = 0;
pub const PACKET_MULTICAST: u8 = 2;
pub const PACKET_LOOPBACK: u8 = 5;
/// `ETH_P_IP`, `ETH_P_IPV6`.
pub const ETH_P_IP: u16 = 0x0800;
pub const ETH_P_IPV6: u16 = 0x86dd;
/// `ETH_HLEN`.
pub const ETH_HLEN: usize = 14;

/// A checked classic BPF program, kept as attached (`orig_prog`) for `SO_GET_FILTER`.
#[derive(Debug, PartialEq, Eq)]
pub struct Program {
    insns: Vec<Insn>,
}

/// `chk_code_allowed`.
fn code_allowed(code: u16) -> bool {
    const ALLOWED: &[u16] = &[
        ALU | ADD | K,
        ALU | ADD | X,
        ALU | SUB | K,
        ALU | SUB | X,
        ALU | MUL | K,
        ALU | MUL | X,
        ALU | DIV | K,
        ALU | DIV | X,
        ALU | MOD | K,
        ALU | MOD | X,
        ALU | AND | K,
        ALU | AND | X,
        ALU | OR | K,
        ALU | OR | X,
        ALU | XOR | K,
        ALU | XOR | X,
        ALU | LSH | K,
        ALU | LSH | X,
        ALU | RSH | K,
        ALU | RSH | X,
        ALU | NEG,
        LD | W | ABS,
        LD | H | ABS,
        LD | B | ABS,
        LD | W | LEN,
        LD | W | IND,
        LD | H | IND,
        LD | B | IND,
        LD | IMM,
        LD | MEM,
        LDX | W | LEN,
        LDX | B | MSH,
        LDX | IMM,
        LDX | MEM,
        ST,
        STX,
        MISC | TAX,
        MISC | TXA,
        RET | K,
        RET | RVAL_A,
        JMP | JA,
        JMP | JEQ | K,
        JMP | JEQ | X,
        JMP | JGE | K,
        JMP | JGE | X,
        JMP | JGT | K,
        JMP | JGT | X,
        JMP | JSET | K,
        JMP | JSET | X,
    ];
    ALLOWED.contains(&code)
}

fn is_cond_jump(code: u16) -> bool {
    code & 0x07 == JMP && code & 0xf0 != JA
}

/// `bpf_anc_helper`: whether `k` names an ancillary load.
fn ancillary(k: u32) -> bool {
    let off = k.wrapping_sub(SKF_AD_OFF as u32) as i32;
    k >= SKF_AD_OFF as u32 && off % 4 == 0 && (AD_PROTOCOL..=AD_VLAN_TPID).contains(&off)
}

impl Program {
    /// `__get_filter`'s checks of the instructions the guest's `sock_fprog` points to
    /// (`bpf_check_basics_ok`, `bpf_check_classic`, `check_load_and_stores`): `EINVAL` for
    /// none, more than `BPF_MAXINSNS`, or a program that is not safe to run.
    pub fn new(raw: &[u8]) -> NetResult<Self> {
        let flen = raw.len() / INSN_LEN;
        if flen == 0 || flen > MAXINSNS || !raw.len().is_multiple_of(INSN_LEN) {
            return Err(Errno::EINVAL);
        }
        let insns: Vec<Insn> = raw
            .chunks_exact(INSN_LEN)
            .map(|c| Insn::read_from(c).expect("a whole instruction"))
            .collect();
        for (pc, i) in insns.iter().enumerate() {
            if !code_allowed(i.code) {
                return Err(Errno::EINVAL);
            }
            let ok = match i.code {
                c if c == ALU | DIV | K || c == ALU | MOD | K => i.k != 0,
                c if c == ALU | LSH | K || c == ALU | RSH | K => i.k < 32,
                c if c == LD | MEM || c == LDX | MEM || c == ST || c == STX => i.k < MEMWORDS,
                c if c == JMP | JA => (i.k as usize) < flen - pc - 1,
                c if is_cond_jump(c) => {
                    pc + usize::from(i.jt) + 1 < flen && pc + usize::from(i.jf) + 1 < flen
                }
                c if c == LD | W | ABS || c == LD | H | ABS || c == LD | B | ABS => {
                    ancillary(i.k) || i.k < SKF_AD_OFF as u32
                }
                _ => true,
            };
            if !ok {
                return Err(Errno::EINVAL);
            }
        }
        let last = insns[flen - 1].code;
        if last != RET | K && last != RET | RVAL_A {
            return Err(Errno::EINVAL);
        }
        check_load_and_stores(&insns)?;
        Ok(Self { insns })
    }

    /// [`Program::new`] for a seccomp filter over a `seccomp_data` of `data_len` bytes, with
    /// `seccomp_check_filter`'s further checks: the only loads from the data are aligned words
    /// inside it (no ancillary or packet-relative loads), a length load reads `data_len`, and
    /// `BPF_MOD`, `BPF_IND` and `BPF_MSH` are refused, all with `EINVAL`.
    pub fn seccomp(raw: &[u8], data_len: u32) -> NetResult<Self> {
        let program = Self::new(raw)?;
        for i in &program.insns {
            let ok = match i.code {
                c if c == LD | W | ABS => i.k < data_len && i.k & 3 == 0,
                c if c == LD | W | LEN || c == LDX | W | LEN => true,
                c if c & 0x07 == ALU => c & 0xf0 != MOD,
                c => {
                    [
                        RET | K,
                        RET | RVAL_A,
                        LD | IMM,
                        LDX | IMM,
                        MISC | TAX,
                        MISC | TXA,
                        LD | MEM,
                        LDX | MEM,
                        ST,
                        STX,
                    ]
                    .contains(&c)
                        || c & 0x07 == JMP
                }
            };
            if !ok {
                return Err(Errno::EINVAL);
            }
        }
        Ok(program)
    }

    /// The program as attached, `struct sock_filter` after `struct sock_filter`.
    pub fn raw(&self) -> Vec<u8> {
        self.insns
            .iter()
            .flat_map(|i| {
                let mut b = [0u8; INSN_LEN];
                i.write_to(&mut b);
                b
            })
            .collect()
    }

    /// The number of instructions.
    pub fn len(&self) -> usize {
        self.insns.len()
    }

    pub fn is_empty(&self) -> bool {
        self.insns.is_empty()
    }

    /// Runs the program over `p` as the eBPF it converts to does (`bpf_convert_filter`): A
    /// and X start at 0, a load out of the packet ends the program with 0, as does a
    /// division by an X of 0; an ancillary load of the device without one ends it with A.
    pub fn run<I: Input + ?Sized>(&self, p: &I) -> u32 {
        let (mut a, mut x) = (0u32, 0u32);
        let mut mem = [0u32; MEMWORDS as usize];
        let mut pc = 0;
        loop {
            let i = self.insns[pc];
            pc += 1;
            let src = match i.code & X {
                X => x,
                _ => i.k,
            };
            match i.code & 0x07 {
                LD | LDX => {
                    let mode = i.code & 0xe0;
                    let size = match i.code & 0x18 {
                        W => 4,
                        H => 2,
                        _ => 1,
                    };
                    let v = match mode {
                        IMM => i.k,
                        MEM => mem[i.k as usize],
                        LEN => p.data_len() as u32,
                        ABS if ancillary(i.k) => match p.ancillary(i.k as i32 - SKF_AD_OFF, a, x) {
                            Some(v) => v,
                            None => return a,
                        },
                        ABS => match p.load(i.k as i32, size) {
                            Some(v) => v,
                            None => return 0,
                        },
                        IND => match p.load(x.wrapping_add(i.k) as i32, size) {
                            Some(v) => v,
                            None => return 0,
                        },
                        // `MSH`: X = 4 * (P[k] & 0xf).
                        _ => match p.load(i.k as i32, 1) {
                            Some(v) => (v & 0xf) << 2,
                            None => return 0,
                        },
                    };
                    match i.code & 0x07 {
                        LD => a = v,
                        _ => x = v,
                    }
                }
                ST => mem[i.k as usize] = a,
                STX => mem[i.k as usize] = x,
                ALU => {
                    a = match i.code & 0xf0 {
                        ADD => a.wrapping_add(src),
                        SUB => a.wrapping_sub(src),
                        MUL => a.wrapping_mul(src),
                        DIV | MOD if src == 0 => return 0,
                        DIV => a / src,
                        MOD => a % src,
                        OR => a | src,
                        AND => a & src,
                        XOR => a ^ src,
                        LSH => a.wrapping_shl(src),
                        RSH => a.wrapping_shr(src),
                        _ => a.wrapping_neg(),
                    }
                }
                JMP => {
                    let skip = match i.code & 0xf0 {
                        JA => i.k as usize,
                        op => {
                            let taken = match op {
                                JEQ => a == src,
                                JGT => a > src,
                                JGE => a >= src,
                                _ => a & src != 0,
                            };
                            usize::from(if taken { i.jt } else { i.jf })
                        }
                    };
                    pc += skip;
                }
                RET => {
                    return match i.code & 0x18 {
                        RVAL_A => a,
                        _ => i.k,
                    };
                }
                _ => match i.code & 0xf8 {
                    TAX => x = a,
                    _ => a = x,
                },
            }
        }
    }
}

/// `check_load_and_stores`: no path reads a memory word it has not written.
fn check_load_and_stores(insns: &[Insn]) -> NetResult<()> {
    let mut masks = vec![u16::MAX; insns.len()];
    let mut valid: u16 = 0;
    for (pc, i) in insns.iter().enumerate() {
        valid &= masks[pc];
        match i.code {
            c if c == ST || c == STX => valid |= 1 << i.k,
            c if c == LD | MEM || c == LDX | MEM => {
                if valid & (1 << i.k) == 0 {
                    return Err(Errno::EINVAL);
                }
            }
            c if c == JMP | JA => {
                masks[pc + 1 + i.k as usize] &= valid;
                valid = u16::MAX;
            }
            c if is_cond_jump(c) => {
                masks[pc + 1 + usize::from(i.jt)] &= valid;
                masks[pc + 1 + usize::from(i.jf)] &= valid;
                valid = u16::MAX;
            }
            _ => {}
        }
    }
    Ok(())
}

/// What a program runs over: a packet, or a `seccomp_data`.
pub trait Input {
    /// `skb->len`: what `BPF_LEN` loads.
    fn data_len(&self) -> usize;

    /// The `size`-byte value at `k` (`BPF_ABS`, `BPF_IND`, `BPF_MSH`), `None` outside.
    fn load(&self, k: i32, size: usize) -> Option<u32>;

    /// The ancillary load `which` (an offset from `SKF_AD_OFF`) with A and X as they are;
    /// `None` ends the program with A.
    fn ancillary(&self, which: i32, a: u32, x: u32) -> Option<u32>;
}

impl Input for Packet<'_> {
    fn data_len(&self) -> usize {
        Packet::len(self)
    }

    fn load(&self, k: i32, size: usize) -> Option<u32> {
        Packet::load(self, k, size)
    }

    fn ancillary(&self, which: i32, a: u32, x: u32) -> Option<u32> {
        Packet::ancillary(self, which, a, x)
    }
}

/// The device a packet arrived on, as the ancillary loads read it.
#[derive(Clone, Copy, Debug)]
pub struct Device {
    pub ifindex: u32,
    /// `dev->type`: `ARPHRD_LOOPBACK` or `ARPHRD_ETHER`.
    pub hatype: u16,
}

/// What a socket filter runs over: the `sk_buff` a socket receives.
#[derive(Clone, Copy, Debug)]
pub struct Packet<'a> {
    /// From the link-layer header (`head`) to the end (`tail`).
    pub frame: &'a [u8],
    /// Where the link-layer header and network header start, and `skb->data`, where what
    /// the socket gets starts; the filter's offsets count from there.
    pub mac: Option<usize>,
    pub network: usize,
    pub data: usize,
    /// `skb_get_poff`: where the payload starts, counted from the data.
    pub pay_offset: u32,
    /// `skb->protocol` (host order), `skb->pkt_type`.
    pub protocol: u16,
    pub pkttype: u8,
    pub dev: Option<Device>,
}

impl Packet<'_> {
    /// `skb->len`.
    fn len(&self) -> usize {
        self.frame.len() - self.data
    }

    /// `bpf_skb_load_helper_*`: a big-endian load at `k` from the data, or below 0 from the
    /// network or link-layer header (`bpf_internal_load_pointer_neg_helper`), anywhere in the
    /// frame.
    fn load(&self, k: i32, size: usize) -> Option<u32> {
        let at = if k >= 0 {
            self.data.checked_add(k as usize)?
        } else if k >= SKF_NET_OFF {
            (self.network as i64 + i64::from(k - SKF_NET_OFF))
                .try_into()
                .ok()?
        } else if k >= SKF_LL_OFF {
            (self.mac? as i64 + i64::from(k - SKF_LL_OFF))
                .try_into()
                .ok()?
        } else {
            return None;
        };
        let bytes = self.frame.get(at..at.checked_add(size)?)?;
        Some(bytes.iter().fold(0u32, |v, b| (v << 8) | u32::from(*b)))
    }

    /// `convert_bpf_extensions` and its helpers; `None` ends the program (a device load
    /// without a device).
    fn ancillary(&self, which: i32, a: u32, x: u32) -> Option<u32> {
        Some(match which {
            AD_PROTOCOL => u32::from(self.protocol),
            AD_PKTTYPE => u32::from(self.pkttype & 7),
            AD_IFINDEX => self.dev?.ifindex,
            AD_HATYPE => u32::from(self.dev?.hatype),
            AD_NLATTR => self.nlattr(a, x, false),
            AD_NLATTR_NEST => self.nlattr(a, x, true),
            AD_ALU_XOR_X => a ^ x,
            AD_PAY_OFFSET => self.pay_offset,
            AD_RANDOM => random_u32(),
            // No marks, queues, hashes, CPUs or VLANs.
            AD_MARK | AD_QUEUE | AD_RXHASH | AD_CPU | AD_VLAN_TAG | AD_VLAN_TAG_PRESENT
            | AD_VLAN_TPID => 0,
            _ => unreachable!("checked by Program::new"),
        })
    }

    /// `bpf_skb_get_nlattr`, `bpf_skb_get_nlattr_nest`: the offset of the netlink attribute
    /// of type `x` among those at offset `a` of the data (or inside the one there), 0 for
    /// none.
    fn nlattr(&self, a: u32, x: u32, nest: bool) -> u32 {
        let data = &self.frame[self.data..];
        let a = a as usize;
        if data.len() < NLA_HDRLEN || a > data.len() - NLA_HDRLEN {
            return 0;
        }
        let (start, len) = match nest {
            false => (a, data.len() - a),
            true => {
                let Some(nla_len) = nla_ok(&data[a..], (data.len() - a) as i64) else {
                    return 0;
                };
                (a + NLA_HDRLEN, nla_len - NLA_HDRLEN)
            }
        };
        nla_find(data, start, len as i64, x).map_or(0, |at| at as u32)
    }
}

/// `NLA_HDRLEN`, `NLA_TYPE_MASK`.
const NLA_HDRLEN: usize = 4;
const NLA_TYPE_MASK: u16 = !(3 << 14);

/// `nla_ok`: the attribute's length, if one fits in `rem` bytes.
fn nla_ok(at: &[u8], rem: i64) -> Option<usize> {
    if rem < NLA_HDRLEN as i64 || at.len() < NLA_HDRLEN {
        return None;
    }
    let len = usize::from(u16::from_le_bytes([at[0], at[1]]));
    (len >= NLA_HDRLEN && len as i64 <= rem).then_some(len)
}

/// `nla_find` over `len` bytes of `data` from `start`.
fn nla_find(data: &[u8], mut at: usize, mut rem: i64, attrtype: u32) -> Option<usize> {
    while let Some(len) = nla_ok(data.get(at..)?, rem) {
        let ty = u16::from_le_bytes([data[at + 2], data[at + 3]]) & NLA_TYPE_MASK;
        if u32::from(ty) == attrtype {
            return Some(at);
        }
        let total = (len + 3) & !3;
        rem -= total as i64;
        at += total;
    }
    None
}

/// `bpf_user_rnd_u32`: a pseudo-random number per call.
fn random_u32() -> u32 {
    static STATE: AtomicU64 = AtomicU64::new(0);
    let mut s = STATE.load(Ordering::Relaxed);
    if s == 0 {
        s = RandomState::new().hash_one(0u8) | 1;
    }
    s ^= s >> 12;
    s ^= s << 25;
    s ^= s >> 27;
    STATE.store(s, Ordering::Relaxed);
    (s.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 32) as u32
}

/// A socket's filter (`sk->sk_filter`) and `SOCK_FILTER_LOCKED`. Clones share the program,
/// as an accepted socket shares its listener's (`sk_clone_lock`).
#[derive(Clone, Debug, Default)]
pub struct SockFilter {
    prog: Option<Arc<Program>>,
    locked: bool,
}

impl SockFilter {
    /// Whether `name` is a `SOL_SOCKET` option of the filter.
    pub fn handles(name: i32) -> bool {
        [
            So::SO_ATTACH_FILTER.0,
            So::SO_DETACH_FILTER.0,
            So::SO_LOCK_FILTER.0,
            So::SO_BPF_EXTENSIONS.0,
        ]
        .contains(&name)
    }

    pub fn is_locked(&self) -> bool {
        self.locked
    }

    /// `sk_setsockopt`'s filter options. `SO_ATTACH_FILTER` takes the program's instructions
    /// (the kernel reads them from the guest's `sock_fprog`): `EPERM` once locked
    /// (`__get_filter`), then the program's checks. `SO_DETACH_FILTER` is `EPERM` once
    /// locked and `ENOENT` without a filter; `SO_LOCK_FILTER` cannot be undone (`EPERM`).
    pub fn set(&mut self, name: i32, value: &[u8]) -> NetResult<()> {
        if name == So::SO_ATTACH_FILTER.0 {
            if self.locked {
                return Err(Errno::EPERM);
            }
            self.prog = Some(Arc::new(Program::new(value)?));
            return Ok(());
        }
        // `sk_setsockopt` reads an `int` first.
        let val = read_int(value)?;
        match So(name) {
            So::SO_DETACH_FILTER => {
                if self.locked {
                    return Err(Errno::EPERM);
                }
                self.prog.take().map(|_| ()).ok_or(Errno::ENOENT)
            }
            So::SO_LOCK_FILTER => {
                if self.locked && val == 0 {
                    return Err(Errno::EPERM);
                }
                self.locked = val != 0;
                Ok(())
            }
            _ => Err(Errno::ENOPROTOOPT),
        }
    }

    /// `sk_getsockopt`: `SO_GET_FILTER` writes the program (nothing without one; the kernel
    /// handles the length, which counts instructions), `SO_LOCK_FILTER` and
    /// `SO_BPF_EXTENSIONS` an `int`.
    pub fn get(&self, name: i32, out: &mut [u8]) -> NetResult<usize> {
        match So(name) {
            So::SO_ATTACH_FILTER => {
                let raw = self.prog.as_ref().map(|p| p.raw()).unwrap_or_default();
                if out.len() < raw.len() {
                    return Err(Errno::EINVAL);
                }
                out[..raw.len()].copy_from_slice(&raw);
                Ok(raw.len())
            }
            So::SO_LOCK_FILTER => Ok(write_int(out, i32::from(self.locked))),
            So::SO_BPF_EXTENSIONS => Ok(write_int(out, SKF_AD_MAX)),
            _ => Err(Errno::ENOPROTOOPT),
        }
    }

    pub fn attached(&self) -> bool {
        self.prog.is_some()
    }

    /// `sk_filter` for a message without network headers or a device (a Unix socket's): how
    /// much of `data` the socket keeps; `None` drops it.
    pub fn trim_message(&self, data: &[u8]) -> Option<usize> {
        let p = Packet {
            frame: data,
            mac: None,
            network: 0,
            data: 0,
            pay_offset: 0,
            protocol: 0,
            pkttype: PACKET_HOST,
            dev: None,
        };
        self.trim(&p, 1)
    }

    /// `sk_filter_trim_cap`: how much of the packet's data the socket keeps, at least `cap`
    /// bytes; `None` drops it.
    pub fn trim(&self, p: &Packet<'_>, cap: usize) -> Option<usize> {
        let Some(prog) = &self.prog else {
            return Some(p.len());
        };
        match prog.run(p) as usize {
            0 => None,
            n => Some(p.len().min(n.max(cap))),
        }
    }
}

// ---- IP packets as a filter sees them ----

/// `ARPHRD_ETHER`, `ARPHRD_LOOPBACK`.
const ARPHRD_ETHER: u16 = 1;
const ARPHRD_LOOPBACK: u16 = 772;

/// How an IP packet arrived: its addresses (canonical), hop limit, interface and
/// `skb->pkt_type`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Arrival {
    pub src: IpAddr,
    pub dst: IpAddr,
    pub ttl: u8,
    pub iif: u32,
    pub pkttype: u8,
}

/// The `sk_buff` of an arrived IP packet ([`Packet`]'s storage).
pub(crate) struct Frame {
    buf: Vec<u8>,
    mac: Option<usize>,
    network: usize,
    data: usize,
    pay_offset: u32,
    protocol: u16,
    pkttype: u8,
    dev: Option<Device>,
}

impl Frame {
    pub fn packet(&self) -> Packet<'_> {
        Packet {
            frame: &self.buf,
            mac: self.mac,
            network: self.network,
            data: self.data,
            pay_offset: self.pay_offset,
            protocol: self.protocol,
            pkttype: self.pkttype,
            dev: self.dev,
        }
    }
}

/// `ip_eth_mc_map`, `ipv6_eth_mc_map`: the link-layer address of a group.
fn group_mac(group: IpAddr) -> [u8; 6] {
    match group {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            [0x01, 0x00, 0x5e, o[1] & 0x7f, o[2], o[3]]
        }
        IpAddr::V6(v6) => {
            let o = v6.octets();
            [0x33, 0x33, o[12], o[13], o[14], o[15]]
        }
    }
}

impl Net {
    /// The frame of a packet that arrived as `at` says, of protocol `proto`: the link-layer
    /// header of the interface it arrived on (none for an interface the namespace does not
    /// have), `ip` (an IP header for `at`'s addresses made up when empty), then `rest`, the
    /// transport header and payload. The socket's data starts at the IP header when
    /// `from_network`, else after it.
    pub(crate) fn frame(
        &self,
        at: &Arrival,
        proto: u8,
        ip: &[u8],
        rest: &[u8],
        from_network: bool,
    ) -> Frame {
        let protocol = match at.dst {
            IpAddr::V4(_) => ETH_P_IP,
            IpAddr::V6(_) => ETH_P_IPV6,
        };
        let link = self.state().ifaces.link(at.iif).map(|l| {
            let hatype = match l.kind {
                LinkKind::Loopback => ARPHRD_LOOPBACK,
                _ => ARPHRD_ETHER,
            };
            (l.mac, hatype)
        });
        let mut buf = Vec::with_capacity(ETH_HLEN + IPV6_HDR + rest.len());
        let mac = link.map(|(mac, _)| {
            let dst = match at.dst.is_multicast() {
                true => group_mac(at.dst),
                false => mac,
            };
            buf.extend(dst);
            buf.extend([0u8; 6]);
            buf.extend(protocol.to_be_bytes());
            0
        });
        let network = buf.len();
        match (ip.is_empty(), at.src, at.dst) {
            (false, ..) => buf.extend_from_slice(ip),
            (true, IpAddr::V4(s), IpAddr::V4(d)) => {
                buf.extend(ipv4_header(s, d, proto, at.ttl, rest.len(), 0))
            }
            (true, s, d) => buf.extend(ipv6_header(v6(s), v6(d), proto, at.ttl, rest.len())),
        }
        let transport = buf.len();
        buf.extend_from_slice(rest);
        let data = if from_network { network } else { transport };
        Frame {
            buf,
            mac,
            network,
            data,
            // Every protocol here has an 8-byte header: UDP, ICMP, ICMPv6.
            pay_offset: (transport - data + 8) as u32,
            protocol,
            pkttype: at.pkttype,
            dev: link.map(|(_, hatype)| Device {
                ifindex: at.iif,
                hatype,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn insn(code: u16, jt: u8, jf: u8, k: u32) -> [u8; 8] {
        let mut b = [0u8; 8];
        b[..2].copy_from_slice(&code.to_le_bytes());
        b[2] = jt;
        b[3] = jf;
        b[4..].copy_from_slice(&k.to_le_bytes());
        b
    }

    fn prog(insns: &[[u8; 8]]) -> NetResult<Program> {
        Program::new(&insns.concat())
    }

    fn packet(frame: &[u8], network: usize, data: usize) -> Packet<'_> {
        Packet {
            frame,
            mac: Some(0),
            network,
            data,
            pay_offset: 0,
            protocol: ETH_P_IP,
            pkttype: PACKET_HOST,
            dev: Some(Device {
                ifindex: 1,
                hatype: 772,
            }),
        }
    }

    #[test]
    fn check_classic_rejects_what_linux_does() {
        let ret = insn(RET | K, 0, 0, 0);
        assert!(prog(&[ret]).is_ok());
        assert_eq!(Program::new(&[]), Err(Errno::EINVAL));
        // Not ending in a return, unknown codes, division by a constant 0, shifts of 32.
        assert_eq!(prog(&[insn(LD | IMM, 0, 0, 1)]), Err(Errno::EINVAL));
        assert_eq!(prog(&[insn(RET | X, 0, 0, 0)]), Err(Errno::EINVAL));
        assert_eq!(
            prog(&[insn(ALU | DIV | K, 0, 0, 0), ret]),
            Err(Errno::EINVAL)
        );
        assert_eq!(
            prog(&[insn(ALU | LSH | K, 0, 0, 32), ret]),
            Err(Errno::EINVAL)
        );
        // Jumps past the end, memory words out of range or read before written.
        assert_eq!(prog(&[insn(JMP | JA, 0, 0, 1), ret]), Err(Errno::EINVAL));
        assert_eq!(
            prog(&[insn(JMP | JEQ | K, 1, 0, 0), ret]),
            Err(Errno::EINVAL)
        );
        assert_eq!(prog(&[insn(ST, 0, 0, 16), ret]), Err(Errno::EINVAL));
        assert_eq!(prog(&[insn(LD | MEM, 0, 0, 3), ret]), Err(Errno::EINVAL));
        // A word written on only one path is not readable after the paths join.
        assert_eq!(
            prog(&[
                insn(JMP | JEQ | K, 0, 1, 0),
                insn(ST, 0, 0, 2),
                insn(LD | MEM, 0, 0, 2),
                ret,
            ]),
            Err(Errno::EINVAL)
        );
        assert!(prog(&[insn(ST, 0, 0, 2), insn(LD | MEM, 0, 0, 2), ret]).is_ok());
        // Unknown ancillary loads; the network and link offsets are plain loads.
        assert_eq!(
            prog(&[insn(LD | W | ABS, 0, 0, (-0x1000i32 + 64) as u32), ret]),
            Err(Errno::EINVAL)
        );
        assert!(prog(&[insn(LD | B | ABS, 0, 0, SKF_NET_OFF as u32), ret]).is_ok());
        let too_long = vec![ret; MAXINSNS + 1];
        assert_eq!(prog(&too_long), Err(Errno::EINVAL));
    }

    #[test]
    fn loads_jumps_and_returns() {
        // IPv4 header, then 4 bytes of data.
        let mut frame = vec![0u8; ETH_HLEN];
        frame.extend([
            0x45, 0, 0, 24, 0, 0, 0, 0, 64, 1, 0, 0, 10, 0, 0, 1, 10, 0, 0, 2,
        ]);
        frame.extend([8, 0, 0xab, 0xcd]);
        let p = packet(&frame, ETH_HLEN, ETH_HLEN + 20);
        // A = data[0]; if A == 8 return LEN else return 0.
        let echo = prog(&[
            insn(LD | B | ABS, 0, 0, 0),
            insn(JMP | JEQ | K, 0, 1, 8),
            insn(LD | W | LEN, 0, 0, 0),
            insn(RET | RVAL_A, 0, 0, 0),
            insn(RET | K, 0, 0, 0),
        ])
        .unwrap();
        assert_eq!(echo.run(&p), 4);
        // The network header's protocol, and X = IP header length via MSH.
        let net = prog(&[
            insn(LD | B | ABS, 0, 0, (SKF_NET_OFF + 9) as u32),
            insn(LDX | B | MSH, 0, 0, (SKF_NET_OFF) as u32),
            insn(ALU | ADD | X, 0, 0, 0),
            insn(RET | RVAL_A, 0, 0, 0),
        ])
        .unwrap();
        // A = protocol 1, X = (0x45 & 0xf) * 4.
        assert_eq!(net.run(&p), 1 + 20);
        // A halfword past the end ends the program with 0.
        let past = prog(&[insn(LD | H | ABS, 0, 0, 3), insn(RET | K, 0, 0, 9)]).unwrap();
        assert_eq!(past.run(&p), 0);
        // Indirect loads, division by X = 0, the ancillary protocol and ifindex.
        let ind = prog(&[
            insn(LDX | IMM, 0, 0, 2),
            insn(LD | H | IND, 0, 0, 0),
            insn(RET | RVAL_A, 0, 0, 0),
        ])
        .unwrap();
        assert_eq!(ind.run(&p), 0xabcd);
        let div = prog(&[
            insn(LD | IMM, 0, 0, 7),
            insn(ALU | DIV | X, 0, 0, 0),
            insn(RET | K, 0, 0, 1),
        ])
        .unwrap();
        assert_eq!(div.run(&p), 0);
        let anc = prog(&[
            insn(LD | W | ABS, 0, 0, (SKF_AD_OFF + AD_PROTOCOL) as u32),
            insn(TAX | MISC, 0, 0, 0),
            insn(LD | W | ABS, 0, 0, (SKF_AD_OFF + AD_IFINDEX) as u32),
            insn(ALU | ADD | X, 0, 0, 0),
            insn(RET | RVAL_A, 0, 0, 0),
        ])
        .unwrap();
        assert_eq!(anc.run(&p), 0x0800 + 1);
        let no_dev = Packet { dev: None, ..p };
        assert_eq!(anc.run(&no_dev), 0x0800);
    }

    #[test]
    fn options_attach_detach_lock() {
        let mut f = SockFilter::default();
        let ret = insn(RET | K, 0, 0, 2);
        assert_eq!(
            f.set(So::SO_DETACH_FILTER.0, &0i32.to_le_bytes()),
            Err(Errno::ENOENT)
        );
        f.set(So::SO_ATTACH_FILTER.0, &ret).unwrap();
        let mut out = [0u8; 8];
        assert_eq!(f.get(SO_GET_FILTER, &mut out), Ok(8));
        assert_eq!(out, ret);
        let frame = [0u8; 10];
        assert_eq!(f.trim(&packet(&frame, 0, 0), 1), Some(2));
        assert_eq!(f.trim(&packet(&frame, 0, 0), 8), Some(8));
        f.set(So::SO_LOCK_FILTER.0, &1i32.to_le_bytes()).unwrap();
        assert_eq!(
            f.set(So::SO_LOCK_FILTER.0, &0i32.to_le_bytes()),
            Err(Errno::EPERM)
        );
        assert_eq!(f.set(So::SO_ATTACH_FILTER.0, &ret), Err(Errno::EPERM));
        assert_eq!(
            f.set(So::SO_DETACH_FILTER.0, &0i32.to_le_bytes()),
            Err(Errno::EPERM)
        );
        assert_eq!(f.set(So::SO_DETACH_FILTER.0, &[0]), Err(Errno::EINVAL));
        let drop_all = SockFilter {
            prog: Some(Arc::new(prog(&[insn(RET | K, 0, 0, 0)]).unwrap())),
            locked: false,
        };
        assert_eq!(drop_all.trim(&packet(&frame, 0, 0), 1), None);
    }
}
