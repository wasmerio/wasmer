//! The only registered generic family: nlctrl (net/netlink/genetlink.c:
//! ctrl_getfamily, ctrl_dumpfamily, ctrl_fill_info). No optional kernel families load.

use wasixv2_abi::{Genlmsghdr, GuestLayout};

use super::*;

impl NetlinkSocket {
    pub(super) fn generic_request(&self, h: &Header, payload: &[u8]) -> Option<Errno> {
        if h.kind != 16 {
            return Some(Errno::ENOENT);
        }
        let Some(genl) = Genlmsghdr::read_from(payload) else {
            return Some(Errno::EINVAL);
        };
        if genl.cmd == 10 {
            return self.controller_policy(h, payload);
        }
        if genl.cmd != 3 {
            return Some(Errno::EOPNOTSUPP);
        }
        let dump = h.flags & NLM_F_DUMP != 0;
        if !dump {
            let mut found = None;
            for (kind, value) in attrs(&payload[Genlmsghdr::SIZE..]) {
                match kind & 0x3fff {
                    1 => {
                        let Some(value) = value.get(..2) else {
                            return Some(Errno::ERANGE);
                        };
                        found = Some(u16::from_le_bytes(value.try_into().unwrap()) == 16);
                    }
                    2 => {
                        if value.is_empty() || value.len() > 16 {
                            return Some(Errno::EINVAL);
                        }
                        let Some(end) = value.iter().position(|c| *c == 0) else {
                            return Some(Errno::EINVAL);
                        };
                        // A name overrides the id, independent of attribute order.
                        found = Some(&value[..end] == b"nlctrl");
                        break;
                    }
                    _ => {}
                }
            }
            match found {
                Some(true) => {}
                Some(false) => return Some(Errno::ENOENT),
                None => return Some(Errno::EINVAL),
            }
        }
        let mut p = vec![1, 2, 0, 0]; // CTRL_CMD_NEWFAMILY; version 2.
        rta(&mut p, 2, b"nlctrl\0");
        rta(&mut p, 1, &16u16.to_le_bytes());
        rta(&mut p, 3, &2u32.to_le_bytes());
        rta(&mut p, 4, &0u32.to_le_bytes());
        rta(&mut p, 5, &0u32.to_le_bytes());
        let mut op = Vec::new();
        rta(&mut op, 1, &3u32.to_le_bytes());
        rta(&mut op, 2, &14u32.to_le_bytes()); // DO | DUMP | HASPOL.
        let mut ops = Vec::new();
        rta(&mut ops, 1, &op);
        let mut policy_op = Vec::new();
        rta(&mut policy_op, 1, &10u32.to_le_bytes());
        rta(&mut policy_op, 2, &12u32.to_le_bytes()); // DUMP | HASPOL.
        rta(&mut ops, 2, &policy_op);
        rta(&mut p, 6, &ops);
        let mut group = Vec::new();
        rta(&mut group, 2, &16u32.to_le_bytes());
        rta(&mut group, 1, b"notify\0");
        let mut groups = Vec::new();
        rta(&mut groups, 1, &group);
        rta(&mut p, 7, &groups);
        let flags = if dump { NLM_F_MULTI } else { 0 };
        let mut reply = nlmsg(16, flags, h.seq, self.port(), &p);
        if dump {
            reply.extend(nlmsg(
                NLMSG_DONE,
                NLM_F_MULTI,
                h.seq,
                self.port(),
                &0i32.to_le_bytes(),
            ));
        }
        self.reply(reply);
        None
    }

    /// net/netlink/genetlink.c: ctrl_dumppolicy_start / ctrl_dumppolicy; policy.c:
    /// __netlink_policy_dump_write_attr. The two nlctrl operations have fixed policies.
    fn controller_policy(&self, h: &Header, payload: &[u8]) -> Option<Errno> {
        if h.flags & NLM_F_DUMP == 0 {
            return Some(Errno::EOPNOTSUPP);
        }
        let mut id = None;
        let mut name = None;
        let mut op = None;
        for (kind, data) in attrs(&payload[Genlmsghdr::SIZE..]) {
            match kind & 0x3fff {
                1 => {
                    let Some(raw) = data.get(..2) else {
                        return Some(Errno::ERANGE);
                    };
                    id = Some(u16::from_le_bytes(raw.try_into().unwrap()));
                }
                2 => {
                    if data.is_empty() || data.len() > 16 || !data.contains(&0) {
                        return Some(Errno::EINVAL);
                    }
                    name = Some(&data[..data.iter().position(|c| *c == 0).unwrap()]);
                }
                10 => {
                    let Some(raw) = data.get(..4) else {
                        return Some(Errno::ERANGE);
                    };
                    op = Some(u32::from_le_bytes(raw.try_into().unwrap()));
                }
                _ => return Some(Errno::EINVAL),
            }
        }
        let found = id
            .map(|id| id == 16)
            .or_else(|| name.map(|name| name == b"nlctrl"));
        match found {
            Some(true) => {}
            Some(false) => return Some(Errno::ENOENT),
            None => return Some(Errno::EINVAL),
        }
        if op.is_some_and(|op| op != 3 && op != 10) {
            return Some(Errno::EOPNOTSUPP);
        }
        let mut reply = Vec::new();
        let mut policies = Vec::new();
        for command in [3u32, 10] {
            if op.is_some_and(|op| op != command) {
                continue;
            }
            let index = if op.is_some() || command == 3 {
                0u32
            } else {
                1u32
            };
            let mut map = Vec::new();
            if command == 3 {
                rta(&mut map, 1, &index.to_le_bytes());
            }
            rta(&mut map, 2, &index.to_le_bytes());
            let mut commands = Vec::new();
            rta(&mut commands, command as u16 | 0x8000, &map);
            let mut p = vec![10, 2, 0, 0];
            rta(&mut p, 1, &16u16.to_le_bytes());
            rta(&mut p, 9 | 0x8000, &commands);
            reply.extend(nlmsg(16, NLM_F_MULTI, h.seq, self.port(), &p));
            let mut policy = Vec::new();
            for (attribute, type_, maximum) in [
                (1u16, 3u32, u64::from(u16::MAX)),
                (2, 12, 15),
                (10, 4, u64::from(u32::MAX)),
            ] {
                if attribute == 10 && command != 10 {
                    continue;
                }
                let mut constraints = Vec::new();
                if type_ == 12 {
                    rta(&mut constraints, 7, &(maximum as u32).to_le_bytes());
                } else {
                    rta(&mut constraints, 4, &0u64.to_le_bytes());
                    rta(&mut constraints, 5, &maximum.to_le_bytes());
                }
                rta(&mut constraints, 1, &type_.to_le_bytes());
                rta(&mut policy, attribute | 0x8000, &constraints);
            }
            rta(&mut policies, index as u16 | 0x8000, &policy);
        }
        let mut p = vec![10, 2, 0, 0];
        rta(&mut p, 1, &16u16.to_le_bytes());
        rta(&mut p, 8 | 0x8000, &policies);
        reply.extend(nlmsg(16, NLM_F_MULTI, h.seq, self.port(), &p));
        reply.extend(nlmsg(
            NLMSG_DONE,
            NLM_F_MULTI,
            h.seq,
            self.port(),
            &0i32.to_le_bytes(),
        ));
        self.reply(reply);
        None
    }
}
