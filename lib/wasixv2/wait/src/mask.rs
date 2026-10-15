use core::{fmt, ops};

use wasixv2_abi::{EpollEvents, PollEvents};

/// A readiness mask: what `Pollable::poll` reports and what `WaitQueue::wake` carries.
///
/// The bits are Linux's `EPOLL*` values, which equal the `POLL*` values on the generic
/// architecture, so converting to and from `PollEvents` and `EpollEvents` keeps every bit
/// (Linux's `mangle_poll` and `demangle_poll` are identities there).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PollMask(pub u32);

impl PollMask {
    pub const IN: Self = Self(0x1);
    pub const PRI: Self = Self(0x2);
    pub const OUT: Self = Self(0x4);
    pub const ERR: Self = Self(0x8);
    pub const HUP: Self = Self(0x10);
    pub const NVAL: Self = Self(0x20);
    pub const RDNORM: Self = Self(0x40);
    pub const RDBAND: Self = Self(0x80);
    pub const WRNORM: Self = Self(0x100);
    pub const WRBAND: Self = Self(0x200);
    pub const MSG: Self = Self(0x400);
    pub const RDHUP: Self = Self(0x2000);

    /// What an object that cannot block reports: Linux's `DEFAULT_POLLMASK`, used for regular
    /// files and anything without its own poll.
    pub const DEFAULT: Self = Self(Self::IN.0 | Self::OUT.0 | Self::RDNORM.0 | Self::WRNORM.0);
    /// Readable: `EPOLLIN | EPOLLRDNORM`, what objects wake readers with.
    pub const READABLE: Self = Self(Self::IN.0 | Self::RDNORM.0);
    /// Writable: `EPOLLOUT | EPOLLWRNORM`, what objects wake writers with.
    pub const WRITABLE: Self = Self(Self::OUT.0 | Self::WRNORM.0);

    pub const fn empty() -> Self {
        Self(0)
    }

    pub const fn bits(self) -> u32 {
        self.0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

impl fmt::Debug for PollMask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const NAMES: &[(&str, PollMask)] = &[
            ("IN", PollMask::IN),
            ("PRI", PollMask::PRI),
            ("OUT", PollMask::OUT),
            ("ERR", PollMask::ERR),
            ("HUP", PollMask::HUP),
            ("NVAL", PollMask::NVAL),
            ("RDNORM", PollMask::RDNORM),
            ("RDBAND", PollMask::RDBAND),
            ("WRNORM", PollMask::WRNORM),
            ("WRBAND", PollMask::WRBAND),
            ("MSG", PollMask::MSG),
            ("RDHUP", PollMask::RDHUP),
        ];
        f.write_str("PollMask(")?;
        let mut rest = self.0;
        for (name, bit) in NAMES {
            if rest & bit.0 != 0 {
                if rest != self.0 {
                    f.write_str(" | ")?;
                }
                f.write_str(name)?;
                rest &= !bit.0;
            }
        }
        if rest != 0 || self.0 == 0 {
            if rest != self.0 {
                f.write_str(" | ")?;
            }
            write!(f, "{rest:#x}")?;
        }
        f.write_str(")")
    }
}

impl ops::BitOr for PollMask {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl ops::BitAnd for PollMask {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

impl ops::Not for PollMask {
    type Output = Self;
    fn not(self) -> Self {
        Self(!self.0)
    }
}

impl ops::BitOrAssign for PollMask {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl ops::BitAndAssign for PollMask {
    fn bitand_assign(&mut self, rhs: Self) {
        self.0 &= rhs.0;
    }
}

impl From<PollEvents> for PollMask {
    fn from(events: PollEvents) -> Self {
        Self(events.0.into())
    }
}

/// Keeps the low 16 bits, which hold every `POLL*` flag.
impl From<PollMask> for PollEvents {
    fn from(mask: PollMask) -> Self {
        Self(mask.0 as u16)
    }
}

impl From<EpollEvents> for PollMask {
    fn from(events: EpollEvents) -> Self {
        Self(events.0)
    }
}

impl From<PollMask> for EpollEvents {
    fn from(mask: PollMask) -> Self {
        Self(mask.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits_match_the_guest_abi() {
        assert_eq!(PollMask::from(PollEvents::POLLRDHUP), PollMask::RDHUP);
        assert_eq!(PollMask::from(EpollEvents::EPOLLWRBAND), PollMask::WRBAND);
        assert_eq!(
            PollEvents::from(PollMask::DEFAULT),
            PollEvents::POLLIN
                | PollEvents::POLLOUT
                | PollEvents::POLLRDNORM
                | PollEvents::POLLWRNORM
        );
        assert_eq!(
            format!("{:?}", PollMask::IN | PollMask(0x4000)),
            "PollMask(IN | 0x4000)"
        );
        assert_eq!(format!("{:?}", PollMask::empty()), "PollMask(0x0)");
    }
}
