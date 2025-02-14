use std::borrow::Cow;
use std::time::SystemTime;
use wasmer_wasix_types::wasi;
use wasmer_wasix_types::wasix::{ThreadStartType, WasiMemoryLayout};

use super::*;

impl From<wasi::Snapshot0Clockid> for JournalSnapshot0ClockidV1 {
    fn from(val: wasi::Snapshot0Clockid) -> Self {
        match val {
            wasi::Snapshot0Clockid::Realtime => JournalSnapshot0ClockidV1::Realtime,
            wasi::Snapshot0Clockid::Monotonic => JournalSnapshot0ClockidV1::Monotonic,
            wasi::Snapshot0Clockid::ProcessCputimeId => JournalSnapshot0ClockidV1::ProcessCputimeId,
            wasi::Snapshot0Clockid::ThreadCputimeId => JournalSnapshot0ClockidV1::ThreadCputimeId,
            wasi::Snapshot0Clockid::Unknown => JournalSnapshot0ClockidV1::Unknown,
        }
    }
}

impl From<JournalSnapshot0ClockidV1> for wasi::Snapshot0Clockid {
    fn from(val: JournalSnapshot0ClockidV1) -> Self {
        match val {
            JournalSnapshot0ClockidV1::Realtime => wasi::Snapshot0Clockid::Realtime,
            JournalSnapshot0ClockidV1::Monotonic => wasi::Snapshot0Clockid::Monotonic,
            JournalSnapshot0ClockidV1::ProcessCputimeId => wasi::Snapshot0Clockid::ProcessCputimeId,
            JournalSnapshot0ClockidV1::ThreadCputimeId => wasi::Snapshot0Clockid::ThreadCputimeId,
            JournalSnapshot0ClockidV1::Unknown => wasi::Snapshot0Clockid::Unknown,
        }
    }
}

impl From<&'_ ArchivedJournalSnapshot0ClockidV1> for wasi::Snapshot0Clockid {
    fn from(val: &'_ ArchivedJournalSnapshot0ClockidV1) -> Self {
        match val {
            ArchivedJournalSnapshot0ClockidV1::Realtime => wasi::Snapshot0Clockid::Realtime,
            ArchivedJournalSnapshot0ClockidV1::Monotonic => wasi::Snapshot0Clockid::Monotonic,
            ArchivedJournalSnapshot0ClockidV1::ProcessCputimeId => {
                wasi::Snapshot0Clockid::ProcessCputimeId
            }
            ArchivedJournalSnapshot0ClockidV1::ThreadCputimeId => {
                wasi::Snapshot0Clockid::ThreadCputimeId
            }
            ArchivedJournalSnapshot0ClockidV1::Unknown => wasi::Snapshot0Clockid::Unknown,
        }
    }
}

impl From<wasi::Whence> for JournalWhenceV1 {
    fn from(val: wasi::Whence) -> Self {
        match val {
            wasi::Whence::Set => JournalWhenceV1::Set,
            wasi::Whence::Cur => JournalWhenceV1::Cur,
            wasi::Whence::End => JournalWhenceV1::End,
            wasi::Whence::Unknown => JournalWhenceV1::Unknown,
        }
    }
}

impl From<JournalWhenceV1> for wasi::Whence {
    fn from(val: JournalWhenceV1) -> Self {
        match val {
            JournalWhenceV1::Set => wasi::Whence::Set,
            JournalWhenceV1::Cur => wasi::Whence::Cur,
            JournalWhenceV1::End => wasi::Whence::End,
            JournalWhenceV1::Unknown => wasi::Whence::Unknown,
        }
    }
}

impl From<&'_ ArchivedJournalWhenceV1> for wasi::Whence {
    fn from(val: &'_ ArchivedJournalWhenceV1) -> Self {
        match val {
            ArchivedJournalWhenceV1::Set => wasi::Whence::Set,
            ArchivedJournalWhenceV1::Cur => wasi::Whence::Cur,
            ArchivedJournalWhenceV1::End => wasi::Whence::End,
            ArchivedJournalWhenceV1::Unknown => wasi::Whence::Unknown,
        }
    }
}

impl From<wasi::Advice> for JournalAdviceV1 {
    fn from(val: wasi::Advice) -> Self {
        match val {
            wasi::Advice::Normal => JournalAdviceV1::Normal,
            wasi::Advice::Sequential => JournalAdviceV1::Sequential,
            wasi::Advice::Random => JournalAdviceV1::Random,
            wasi::Advice::Willneed => JournalAdviceV1::Willneed,
            wasi::Advice::Dontneed => JournalAdviceV1::Dontneed,
            wasi::Advice::Noreuse => JournalAdviceV1::Noreuse,
            wasi::Advice::Unknown => JournalAdviceV1::Unknown,
        }
    }
}

impl From<JournalAdviceV1> for wasi::Advice {
    fn from(val: JournalAdviceV1) -> Self {
        match val {
            JournalAdviceV1::Normal => wasi::Advice::Normal,
            JournalAdviceV1::Sequential => wasi::Advice::Sequential,
            JournalAdviceV1::Random => wasi::Advice::Random,
            JournalAdviceV1::Willneed => wasi::Advice::Willneed,
            JournalAdviceV1::Dontneed => wasi::Advice::Dontneed,
            JournalAdviceV1::Noreuse => wasi::Advice::Noreuse,
            JournalAdviceV1::Unknown => wasi::Advice::Unknown,
        }
    }
}

impl From<&'_ ArchivedJournalAdviceV1> for wasi::Advice {
    fn from(val: &'_ ArchivedJournalAdviceV1) -> Self {
        match val {
            ArchivedJournalAdviceV1::Normal => wasi::Advice::Normal,
            ArchivedJournalAdviceV1::Sequential => wasi::Advice::Sequential,
            ArchivedJournalAdviceV1::Random => wasi::Advice::Random,
            ArchivedJournalAdviceV1::Willneed => wasi::Advice::Willneed,
            ArchivedJournalAdviceV1::Dontneed => wasi::Advice::Dontneed,
            ArchivedJournalAdviceV1::Noreuse => wasi::Advice::Noreuse,
            ArchivedJournalAdviceV1::Unknown => wasi::Advice::Unknown,
        }
    }
}

impl From<virtual_net::IpCidr> for JournalIpCidrV1 {
    fn from(value: virtual_net::IpCidr) -> Self {
        Self {
            ip: value.ip,
            prefix: value.prefix,
        }
    }
}

impl From<JournalIpCidrV1> for virtual_net::IpCidr {
    fn from(value: JournalIpCidrV1) -> Self {
        Self {
            ip: value.ip,
            prefix: value.prefix,
        }
    }
}

impl From<wasi::ExitCode> for JournalExitCodeV1 {
    fn from(val: wasi::ExitCode) -> Self {
        JournalExitCodeV1::Other(val.raw())
    }
}

impl From<JournalExitCodeV1> for wasi::ExitCode {
    fn from(val: JournalExitCodeV1) -> Self {
        match val {
            JournalExitCodeV1::Errno(errno) => wasi::ExitCode::from(errno),
            JournalExitCodeV1::Other(id) => wasi::ExitCode::from(id),
        }
    }
}

impl From<&'_ ArchivedJournalExitCodeV1> for wasi::ExitCode {
    fn from(val: &'_ ArchivedJournalExitCodeV1) -> Self {
        match val {
            ArchivedJournalExitCodeV1::Errno(errno) => wasi::ExitCode::from(errno.to_native()),
            ArchivedJournalExitCodeV1::Other(id) => wasi::ExitCode::from(id.to_native()),
        }
    }
}

impl From<SnapshotTrigger> for JournalSnapshotTriggerV1 {
    fn from(val: SnapshotTrigger) -> Self {
        match val {
            SnapshotTrigger::Idle => JournalSnapshotTriggerV1::Idle,
            SnapshotTrigger::FirstListen => JournalSnapshotTriggerV1::Listen,
            SnapshotTrigger::FirstEnviron => JournalSnapshotTriggerV1::Environ,
            SnapshotTrigger::FirstStdin => JournalSnapshotTriggerV1::Stdin,
            SnapshotTrigger::FirstSigint => JournalSnapshotTriggerV1::Sigint,
            SnapshotTrigger::PeriodicInterval => JournalSnapshotTriggerV1::Timer,
            SnapshotTrigger::Sigint => JournalSnapshotTriggerV1::Sigint,
            SnapshotTrigger::Sigalrm => JournalSnapshotTriggerV1::Sigalrm,
            SnapshotTrigger::Sigtstp => JournalSnapshotTriggerV1::Sigtstp,
            SnapshotTrigger::Sigstop => JournalSnapshotTriggerV1::Sigstop,
            SnapshotTrigger::NonDeterministicCall => JournalSnapshotTriggerV1::NonDeterministicCall,
            SnapshotTrigger::Bootstrap => JournalSnapshotTriggerV1::Bootstrap,
            SnapshotTrigger::Transaction => JournalSnapshotTriggerV1::Transaction,
            SnapshotTrigger::Explicit => JournalSnapshotTriggerV1::Explicit,
        }
    }
}

impl From<JournalSnapshotTriggerV1> for SnapshotTrigger {
    fn from(val: JournalSnapshotTriggerV1) -> Self {
        match val {
            JournalSnapshotTriggerV1::Idle => SnapshotTrigger::Idle,
            JournalSnapshotTriggerV1::Listen => SnapshotTrigger::FirstListen,
            JournalSnapshotTriggerV1::Environ => SnapshotTrigger::FirstEnviron,
            JournalSnapshotTriggerV1::Stdin => SnapshotTrigger::FirstStdin,
            JournalSnapshotTriggerV1::Timer => SnapshotTrigger::PeriodicInterval,
            JournalSnapshotTriggerV1::Sigint => SnapshotTrigger::Sigint,
            JournalSnapshotTriggerV1::Sigalrm => SnapshotTrigger::Sigalrm,
            JournalSnapshotTriggerV1::Sigtstp => SnapshotTrigger::Sigtstp,
            JournalSnapshotTriggerV1::Sigstop => SnapshotTrigger::Sigstop,
            JournalSnapshotTriggerV1::NonDeterministicCall => SnapshotTrigger::NonDeterministicCall,
            JournalSnapshotTriggerV1::Bootstrap => SnapshotTrigger::Bootstrap,
            JournalSnapshotTriggerV1::Transaction => SnapshotTrigger::Transaction,
            JournalSnapshotTriggerV1::Explicit => SnapshotTrigger::Explicit,
        }
    }
}

impl From<&'_ ArchivedJournalSnapshotTriggerV1> for SnapshotTrigger {
    fn from(val: &'_ ArchivedJournalSnapshotTriggerV1) -> Self {
        match val {
            ArchivedJournalSnapshotTriggerV1::Idle => SnapshotTrigger::Idle,
            ArchivedJournalSnapshotTriggerV1::Listen => SnapshotTrigger::FirstListen,
            ArchivedJournalSnapshotTriggerV1::Environ => SnapshotTrigger::FirstEnviron,
            ArchivedJournalSnapshotTriggerV1::Stdin => SnapshotTrigger::FirstStdin,
            ArchivedJournalSnapshotTriggerV1::Timer => SnapshotTrigger::PeriodicInterval,
            ArchivedJournalSnapshotTriggerV1::Sigint => SnapshotTrigger::Sigint,
            ArchivedJournalSnapshotTriggerV1::Sigalrm => SnapshotTrigger::Sigalrm,
            ArchivedJournalSnapshotTriggerV1::Sigtstp => SnapshotTrigger::Sigtstp,
            ArchivedJournalSnapshotTriggerV1::Sigstop => SnapshotTrigger::Sigstop,
            ArchivedJournalSnapshotTriggerV1::NonDeterministicCall => {
                SnapshotTrigger::NonDeterministicCall
            }
            ArchivedJournalSnapshotTriggerV1::Bootstrap => SnapshotTrigger::Bootstrap,
            ArchivedJournalSnapshotTriggerV1::Transaction => SnapshotTrigger::Transaction,
            ArchivedJournalSnapshotTriggerV1::Explicit => SnapshotTrigger::Explicit,
        }
    }
}

impl From<wasi::EpollCtl> for JournalEpollCtlV1 {
    fn from(val: wasi::EpollCtl) -> Self {
        match val {
            wasi::EpollCtl::Add => JournalEpollCtlV1::Add,
            wasi::EpollCtl::Mod => JournalEpollCtlV1::Mod,
            wasi::EpollCtl::Del => JournalEpollCtlV1::Del,
            wasi::EpollCtl::Unknown => JournalEpollCtlV1::Unknown,
        }
    }
}

impl From<JournalEpollCtlV1> for wasi::EpollCtl {
    fn from(val: JournalEpollCtlV1) -> Self {
        match val {
            JournalEpollCtlV1::Add => wasi::EpollCtl::Add,
            JournalEpollCtlV1::Mod => wasi::EpollCtl::Mod,
            JournalEpollCtlV1::Del => wasi::EpollCtl::Del,
            JournalEpollCtlV1::Unknown => wasi::EpollCtl::Unknown,
        }
    }
}

impl From<&'_ ArchivedJournalEpollCtlV1> for wasi::EpollCtl {
    fn from(val: &'_ ArchivedJournalEpollCtlV1) -> Self {
        match val {
            ArchivedJournalEpollCtlV1::Add => wasi::EpollCtl::Add,
            ArchivedJournalEpollCtlV1::Mod => wasi::EpollCtl::Mod,
            ArchivedJournalEpollCtlV1::Del => wasi::EpollCtl::Del,
            ArchivedJournalEpollCtlV1::Unknown => wasi::EpollCtl::Unknown,
        }
    }
}

impl From<wasi::EpollEventCtl> for JournalEpollEventCtlV1 {
    fn from(val: wasi::EpollEventCtl) -> Self {
        JournalEpollEventCtlV1 {
            events: val.events.bits(),
            ptr: val.ptr,
            fd: val.fd,
            data1: val.data1,
            data2: val.data2,
        }
    }
}

impl From<JournalEpollEventCtlV1> for wasi::EpollEventCtl {
    fn from(val: JournalEpollEventCtlV1) -> Self {
        Self {
            events: wasi::EpollType::from_bits_truncate(val.events),
            ptr: val.ptr,
            fd: val.fd,
            data1: val.data1,
            data2: val.data2,
        }
    }
}

impl From<&'_ ArchivedJournalEpollEventCtlV1> for wasi::EpollEventCtl {
    fn from(val: &'_ ArchivedJournalEpollEventCtlV1) -> Self {
        Self {
            events: wasi::EpollType::from_bits_truncate(val.events.to_native()),
            ptr: val.ptr.to_native(),
            fd: val.fd.to_native(),
            data1: val.data1.to_native(),
            data2: val.data2.to_native(),
        }
    }
}

impl From<virtual_net::StreamSecurity> for JournalStreamSecurityV1 {
    fn from(val: virtual_net::StreamSecurity) -> Self {
        use virtual_net::StreamSecurity;
        match val {
            StreamSecurity::Unencrypted => JournalStreamSecurityV1::Unencrypted,
            StreamSecurity::AnyEncyption => JournalStreamSecurityV1::AnyEncryption,
            StreamSecurity::ClassicEncryption => JournalStreamSecurityV1::ClassicEncryption,
            StreamSecurity::DoubleEncryption => JournalStreamSecurityV1::DoubleEncryption,
        }
    }
}

impl From<JournalStreamSecurityV1> for virtual_net::StreamSecurity {
    fn from(val: JournalStreamSecurityV1) -> Self {
        use virtual_net::StreamSecurity;
        match val {
            JournalStreamSecurityV1::Unencrypted => StreamSecurity::Unencrypted,
            JournalStreamSecurityV1::AnyEncryption => StreamSecurity::AnyEncyption,
            JournalStreamSecurityV1::ClassicEncryption => StreamSecurity::ClassicEncryption,
            JournalStreamSecurityV1::DoubleEncryption => StreamSecurity::DoubleEncryption,
            JournalStreamSecurityV1::Unknown => StreamSecurity::AnyEncyption,
        }
    }
}

impl From<&'_ ArchivedJournalStreamSecurityV1> for virtual_net::StreamSecurity {
    fn from(val: &'_ ArchivedJournalStreamSecurityV1) -> Self {
        use virtual_net::StreamSecurity;
        match val {
            ArchivedJournalStreamSecurityV1::Unencrypted => StreamSecurity::Unencrypted,
            ArchivedJournalStreamSecurityV1::AnyEncryption => StreamSecurity::AnyEncyption,
            ArchivedJournalStreamSecurityV1::ClassicEncryption => StreamSecurity::ClassicEncryption,
            ArchivedJournalStreamSecurityV1::DoubleEncryption => StreamSecurity::DoubleEncryption,
            ArchivedJournalStreamSecurityV1::Unknown => StreamSecurity::AnyEncyption,
        }
    }
}

impl From<wasi::Addressfamily> for JournalAddressfamilyV1 {
    fn from(val: wasi::Addressfamily) -> Self {
        match val {
            wasi::Addressfamily::Unspec => JournalAddressfamilyV1::Unspec,
            wasi::Addressfamily::Inet4 => JournalAddressfamilyV1::Inet4,
            wasi::Addressfamily::Inet6 => JournalAddressfamilyV1::Inet6,
            wasi::Addressfamily::Unix => JournalAddressfamilyV1::Unix,
        }
    }
}

impl From<JournalAddressfamilyV1> for wasi::Addressfamily {
    fn from(val: JournalAddressfamilyV1) -> Self {
        match val {
            JournalAddressfamilyV1::Unspec => wasi::Addressfamily::Unspec,
            JournalAddressfamilyV1::Inet4 => wasi::Addressfamily::Inet4,
            JournalAddressfamilyV1::Inet6 => wasi::Addressfamily::Inet6,
            JournalAddressfamilyV1::Unix => wasi::Addressfamily::Unix,
        }
    }
}

impl From<&'_ ArchivedJournalAddressfamilyV1> for wasi::Addressfamily {
    fn from(val: &'_ ArchivedJournalAddressfamilyV1) -> Self {
        match val {
            ArchivedJournalAddressfamilyV1::Unspec => wasi::Addressfamily::Unspec,
            ArchivedJournalAddressfamilyV1::Inet4 => wasi::Addressfamily::Inet4,
            ArchivedJournalAddressfamilyV1::Inet6 => wasi::Addressfamily::Inet6,
            ArchivedJournalAddressfamilyV1::Unix => wasi::Addressfamily::Unix,
        }
    }
}

impl From<wasi::Socktype> for JournalSocktypeV1 {
    fn from(val: wasi::Socktype) -> Self {
        match val {
            wasi::Socktype::Stream => JournalSocktypeV1::Stream,
            wasi::Socktype::Dgram => JournalSocktypeV1::Dgram,
            wasi::Socktype::Raw => JournalSocktypeV1::Raw,
            wasi::Socktype::Seqpacket => JournalSocktypeV1::Seqpacket,
            wasi::Socktype::Unknown => JournalSocktypeV1::Unknown,
        }
    }
}

impl From<JournalSocktypeV1> for wasi::Socktype {
    fn from(val: JournalSocktypeV1) -> Self {
        match val {
            JournalSocktypeV1::Stream => wasi::Socktype::Stream,
            JournalSocktypeV1::Dgram => wasi::Socktype::Dgram,
            JournalSocktypeV1::Raw => wasi::Socktype::Raw,
            JournalSocktypeV1::Seqpacket => wasi::Socktype::Seqpacket,
            JournalSocktypeV1::Unknown => wasi::Socktype::Unknown,
        }
    }
}

impl From<&'_ ArchivedJournalSocktypeV1> for wasi::Socktype {
    fn from(val: &'_ ArchivedJournalSocktypeV1) -> Self {
        match val {
            ArchivedJournalSocktypeV1::Stream => wasi::Socktype::Stream,
            ArchivedJournalSocktypeV1::Dgram => wasi::Socktype::Dgram,
            ArchivedJournalSocktypeV1::Raw => wasi::Socktype::Raw,
            ArchivedJournalSocktypeV1::Seqpacket => wasi::Socktype::Seqpacket,
            ArchivedJournalSocktypeV1::Unknown => wasi::Socktype::Unknown,
        }
    }
}

impl From<wasi::Sockoption> for JournalSockoptionV1 {
    fn from(val: wasi::Sockoption) -> Self {
        match val {
            wasi::Sockoption::Noop => JournalSockoptionV1::Noop,
            wasi::Sockoption::ReusePort => JournalSockoptionV1::ReusePort,
            wasi::Sockoption::ReuseAddr => JournalSockoptionV1::ReuseAddr,
            wasi::Sockoption::NoDelay => JournalSockoptionV1::NoDelay,
            wasi::Sockoption::DontRoute => JournalSockoptionV1::DontRoute,
            wasi::Sockoption::OnlyV6 => JournalSockoptionV1::OnlyV6,
            wasi::Sockoption::Broadcast => JournalSockoptionV1::Broadcast,
            wasi::Sockoption::MulticastLoopV4 => JournalSockoptionV1::MulticastLoopV4,
            wasi::Sockoption::MulticastLoopV6 => JournalSockoptionV1::MulticastLoopV6,
            wasi::Sockoption::Promiscuous => JournalSockoptionV1::Promiscuous,
            wasi::Sockoption::Listening => JournalSockoptionV1::Listening,
            wasi::Sockoption::LastError => JournalSockoptionV1::LastError,
            wasi::Sockoption::KeepAlive => JournalSockoptionV1::KeepAlive,
            wasi::Sockoption::Linger => JournalSockoptionV1::Linger,
            wasi::Sockoption::OobInline => JournalSockoptionV1::OobInline,
            wasi::Sockoption::RecvBufSize => JournalSockoptionV1::RecvBufSize,
            wasi::Sockoption::SendBufSize => JournalSockoptionV1::SendBufSize,
            wasi::Sockoption::RecvLowat => JournalSockoptionV1::RecvLowat,
            wasi::Sockoption::SendLowat => JournalSockoptionV1::SendLowat,
            wasi::Sockoption::RecvTimeout => JournalSockoptionV1::RecvTimeout,
            wasi::Sockoption::SendTimeout => JournalSockoptionV1::SendTimeout,
            wasi::Sockoption::ConnectTimeout => JournalSockoptionV1::ConnectTimeout,
            wasi::Sockoption::AcceptTimeout => JournalSockoptionV1::AcceptTimeout,
            wasi::Sockoption::Ttl => JournalSockoptionV1::Ttl,
            wasi::Sockoption::MulticastTtlV4 => JournalSockoptionV1::MulticastTtlV4,
            wasi::Sockoption::Type => JournalSockoptionV1::Type,
            wasi::Sockoption::Proto => JournalSockoptionV1::Proto,
        }
    }
}

impl From<JournalSockoptionV1> for wasi::Sockoption {
    fn from(val: JournalSockoptionV1) -> Self {
        match val {
            JournalSockoptionV1::Noop => wasi::Sockoption::Noop,
            JournalSockoptionV1::ReusePort => wasi::Sockoption::ReusePort,
            JournalSockoptionV1::ReuseAddr => wasi::Sockoption::ReuseAddr,
            JournalSockoptionV1::NoDelay => wasi::Sockoption::NoDelay,
            JournalSockoptionV1::DontRoute => wasi::Sockoption::DontRoute,
            JournalSockoptionV1::OnlyV6 => wasi::Sockoption::OnlyV6,
            JournalSockoptionV1::Broadcast => wasi::Sockoption::Broadcast,
            JournalSockoptionV1::MulticastLoopV4 => wasi::Sockoption::MulticastLoopV4,
            JournalSockoptionV1::MulticastLoopV6 => wasi::Sockoption::MulticastLoopV6,
            JournalSockoptionV1::Promiscuous => wasi::Sockoption::Promiscuous,
            JournalSockoptionV1::Listening => wasi::Sockoption::Listening,
            JournalSockoptionV1::LastError => wasi::Sockoption::LastError,
            JournalSockoptionV1::KeepAlive => wasi::Sockoption::KeepAlive,
            JournalSockoptionV1::Linger => wasi::Sockoption::Linger,
            JournalSockoptionV1::OobInline => wasi::Sockoption::OobInline,
            JournalSockoptionV1::RecvBufSize => wasi::Sockoption::RecvBufSize,
            JournalSockoptionV1::SendBufSize => wasi::Sockoption::SendBufSize,
            JournalSockoptionV1::RecvLowat => wasi::Sockoption::RecvLowat,
            JournalSockoptionV1::SendLowat => wasi::Sockoption::SendLowat,
            JournalSockoptionV1::RecvTimeout => wasi::Sockoption::RecvTimeout,
            JournalSockoptionV1::SendTimeout => wasi::Sockoption::SendTimeout,
            JournalSockoptionV1::ConnectTimeout => wasi::Sockoption::ConnectTimeout,
            JournalSockoptionV1::AcceptTimeout => wasi::Sockoption::AcceptTimeout,
            JournalSockoptionV1::Ttl => wasi::Sockoption::Ttl,
            JournalSockoptionV1::MulticastTtlV4 => wasi::Sockoption::MulticastTtlV4,
            JournalSockoptionV1::Type => wasi::Sockoption::Type,
            JournalSockoptionV1::Proto => wasi::Sockoption::Proto,
        }
    }
}

impl From<&'_ ArchivedJournalSockoptionV1> for wasi::Sockoption {
    fn from(val: &'_ ArchivedJournalSockoptionV1) -> Self {
        match val {
            ArchivedJournalSockoptionV1::Noop => wasi::Sockoption::Noop,
            ArchivedJournalSockoptionV1::ReusePort => wasi::Sockoption::ReusePort,
            ArchivedJournalSockoptionV1::ReuseAddr => wasi::Sockoption::ReuseAddr,
            ArchivedJournalSockoptionV1::NoDelay => wasi::Sockoption::NoDelay,
            ArchivedJournalSockoptionV1::DontRoute => wasi::Sockoption::DontRoute,
            ArchivedJournalSockoptionV1::OnlyV6 => wasi::Sockoption::OnlyV6,
            ArchivedJournalSockoptionV1::Broadcast => wasi::Sockoption::Broadcast,
            ArchivedJournalSockoptionV1::MulticastLoopV4 => wasi::Sockoption::MulticastLoopV4,
            ArchivedJournalSockoptionV1::MulticastLoopV6 => wasi::Sockoption::MulticastLoopV6,
            ArchivedJournalSockoptionV1::Promiscuous => wasi::Sockoption::Promiscuous,
            ArchivedJournalSockoptionV1::Listening => wasi::Sockoption::Listening,
            ArchivedJournalSockoptionV1::LastError => wasi::Sockoption::LastError,
            ArchivedJournalSockoptionV1::KeepAlive => wasi::Sockoption::KeepAlive,
            ArchivedJournalSockoptionV1::Linger => wasi::Sockoption::Linger,
            ArchivedJournalSockoptionV1::OobInline => wasi::Sockoption::OobInline,
            ArchivedJournalSockoptionV1::RecvBufSize => wasi::Sockoption::RecvBufSize,
            ArchivedJournalSockoptionV1::SendBufSize => wasi::Sockoption::SendBufSize,
            ArchivedJournalSockoptionV1::RecvLowat => wasi::Sockoption::RecvLowat,
            ArchivedJournalSockoptionV1::SendLowat => wasi::Sockoption::SendLowat,
            ArchivedJournalSockoptionV1::RecvTimeout => wasi::Sockoption::RecvTimeout,
            ArchivedJournalSockoptionV1::SendTimeout => wasi::Sockoption::SendTimeout,
            ArchivedJournalSockoptionV1::ConnectTimeout => wasi::Sockoption::ConnectTimeout,
            ArchivedJournalSockoptionV1::AcceptTimeout => wasi::Sockoption::AcceptTimeout,
            ArchivedJournalSockoptionV1::Ttl => wasi::Sockoption::Ttl,
            ArchivedJournalSockoptionV1::MulticastTtlV4 => wasi::Sockoption::MulticastTtlV4,
            ArchivedJournalSockoptionV1::Type => wasi::Sockoption::Type,
            ArchivedJournalSockoptionV1::Proto => wasi::Sockoption::Proto,
        }
    }
}

impl From<SocketOptTimeType> for JournalTimeTypeV1 {
    fn from(val: SocketOptTimeType) -> Self {
        match val {
            SocketOptTimeType::ReadTimeout => JournalTimeTypeV1::ReadTimeout,
            SocketOptTimeType::WriteTimeout => JournalTimeTypeV1::WriteTimeout,
            SocketOptTimeType::AcceptTimeout => JournalTimeTypeV1::AcceptTimeout,
            SocketOptTimeType::ConnectTimeout => JournalTimeTypeV1::ConnectTimeout,
            SocketOptTimeType::BindTimeout => JournalTimeTypeV1::BindTimeout,
            SocketOptTimeType::Linger => JournalTimeTypeV1::Linger,
        }
    }
}

impl From<JournalTimeTypeV1> for SocketOptTimeType {
    fn from(val: JournalTimeTypeV1) -> Self {
        match val {
            JournalTimeTypeV1::ReadTimeout => SocketOptTimeType::ReadTimeout,
            JournalTimeTypeV1::WriteTimeout => SocketOptTimeType::WriteTimeout,
            JournalTimeTypeV1::AcceptTimeout => SocketOptTimeType::AcceptTimeout,
            JournalTimeTypeV1::ConnectTimeout => SocketOptTimeType::ConnectTimeout,
            JournalTimeTypeV1::BindTimeout => SocketOptTimeType::BindTimeout,
            JournalTimeTypeV1::Linger => SocketOptTimeType::Linger,
        }
    }
}

impl From<&'_ ArchivedJournalTimeTypeV1> for SocketOptTimeType {
    fn from(val: &'_ ArchivedJournalTimeTypeV1) -> Self {
        match val {
            ArchivedJournalTimeTypeV1::ReadTimeout => SocketOptTimeType::ReadTimeout,
            ArchivedJournalTimeTypeV1::WriteTimeout => SocketOptTimeType::WriteTimeout,
            ArchivedJournalTimeTypeV1::AcceptTimeout => SocketOptTimeType::AcceptTimeout,
            ArchivedJournalTimeTypeV1::ConnectTimeout => SocketOptTimeType::ConnectTimeout,
            ArchivedJournalTimeTypeV1::BindTimeout => SocketOptTimeType::BindTimeout,
            ArchivedJournalTimeTypeV1::Linger => SocketOptTimeType::Linger,
        }
    }
}

impl From<SocketShutdownHow> for JournalSocketShutdownV1 {
    fn from(val: SocketShutdownHow) -> Self {
        match val {
            SocketShutdownHow::Read => JournalSocketShutdownV1::Read,
            SocketShutdownHow::Write => JournalSocketShutdownV1::Write,
            SocketShutdownHow::Both => JournalSocketShutdownV1::Both,
        }
    }
}

impl From<JournalSocketShutdownV1> for SocketShutdownHow {
    fn from(val: JournalSocketShutdownV1) -> Self {
        match val {
            JournalSocketShutdownV1::Read => SocketShutdownHow::Read,
            JournalSocketShutdownV1::Write => SocketShutdownHow::Write,
            JournalSocketShutdownV1::Both => SocketShutdownHow::Both,
        }
    }
}

impl From<&'_ ArchivedJournalSocketShutdownV1> for SocketShutdownHow {
    fn from(val: &'_ ArchivedJournalSocketShutdownV1) -> Self {
        match val {
            ArchivedJournalSocketShutdownV1::Read => SocketShutdownHow::Read,
            ArchivedJournalSocketShutdownV1::Write => SocketShutdownHow::Write,
            ArchivedJournalSocketShutdownV1::Both => SocketShutdownHow::Both,
        }
    }
}

impl From<JournalThreadStartTypeV1> for ThreadStartType {
    fn from(value: JournalThreadStartTypeV1) -> Self {
        match value {
            JournalThreadStartTypeV1::MainThread => ThreadStartType::MainThread,
            JournalThreadStartTypeV1::ThreadSpawn { start_ptr } => {
                ThreadStartType::ThreadSpawn { start_ptr }
            }
        }
    }
}

impl From<&'_ ArchivedJournalThreadStartTypeV1> for ThreadStartType {
    fn from(value: &'_ ArchivedJournalThreadStartTypeV1) -> Self {
        match value {
            ArchivedJournalThreadStartTypeV1::MainThread => ThreadStartType::MainThread,
            ArchivedJournalThreadStartTypeV1::ThreadSpawn { start_ptr } => {
                ThreadStartType::ThreadSpawn {
                    start_ptr: start_ptr.to_native(),
                }
            }
        }
    }
}

impl From<ThreadStartType> for JournalThreadStartTypeV1 {
    fn from(value: ThreadStartType) -> Self {
        match value {
            ThreadStartType::MainThread => JournalThreadStartTypeV1::MainThread,
            ThreadStartType::ThreadSpawn { start_ptr } => {
                JournalThreadStartTypeV1::ThreadSpawn { start_ptr }
            }
        }
    }
}

impl From<JournalWasiMemoryLayout> for WasiMemoryLayout {
    fn from(value: JournalWasiMemoryLayout) -> Self {
        Self {
            stack_upper: value.stack_upper,
            stack_lower: value.stack_lower,
            guard_size: value.guard_size,
            stack_size: value.stack_size,
        }
    }
}

impl From<&'_ ArchivedJournalWasiMemoryLayout> for WasiMemoryLayout {
    fn from(value: &'_ ArchivedJournalWasiMemoryLayout) -> Self {
        Self {
            stack_upper: value.stack_upper.to_native(),
            stack_lower: value.stack_lower.to_native(),
            guard_size: value.guard_size.to_native(),
            stack_size: value.stack_size.to_native(),
        }
    }
}

impl From<WasiMemoryLayout> for JournalWasiMemoryLayout {
    fn from(value: WasiMemoryLayout) -> Self {
        Self {
            stack_upper: value.stack_upper,
            stack_lower: value.stack_lower,
            guard_size: value.guard_size,
            stack_size: value.stack_size,
        }
    }
}

impl<'a> TryFrom<ArchivedJournalEntry<'a>> for JournalEntry<'a> {
    type Error = anyhow::Error;

    fn try_from(value: ArchivedJournalEntry<'a>) -> anyhow::Result<Self> {
        Ok(match value {
            ArchivedJournalEntry::InitModuleV1(ArchivedJournalEntryInitModuleV1 { wasm_hash }) => {
                Self::InitModuleV1 {
                    wasm_hash: Box::from(wasm_hash.get()),
                }
            }
            ArchivedJournalEntry::ClearEtherealV1(ArchivedJournalEntryClearEtherealV1 {
                ..
            }) => Self::ClearEtherealV1,
            ArchivedJournalEntry::UpdateMemoryRegionV1(
                ArchivedJournalEntryUpdateMemoryRegionV1 {
                    start,
                    end,
                    compressed_data,
                },
            ) => Self::UpdateMemoryRegionV1 {
                region: (start.to_native())..(end.to_native()),
                compressed_data: Cow::Borrowed(compressed_data.as_ref()),
            },
            ArchivedJournalEntry::ProcessExitV1(ArchivedJournalEntryProcessExitV1 {
                exit_code,
            }) => Self::ProcessExitV1 {
                exit_code: exit_code.as_ref().map(|code| code.into()),
            },
            ArchivedJournalEntry::SetThreadV1(ArchivedJournalEntrySetThreadV1 {
                id,
                call_stack,
                memory_stack,
                store_data,
                is_64bit,
                start,
                layout,
            }) => Self::SetThreadV1 {
                id: id.to_native(),
                call_stack: call_stack.as_ref().into(),
                memory_stack: memory_stack.as_ref().into(),
                store_data: store_data.as_ref().into(),
                start: start.into(),
                layout: layout.into(),
                is_64bit: *is_64bit,
            },
            ArchivedJournalEntry::CloseThreadV1(ArchivedJournalEntryCloseThreadV1 {
                id,
                exit_code,
            }) => Self::CloseThreadV1 {
                id: id.to_native(),
                exit_code: exit_code.as_ref().map(|code| code.into()),
            },
            ArchivedJournalEntry::FileDescriptorWriteV1(
                ArchivedJournalEntryFileDescriptorWriteV1 {
                    data,
                    fd,
                    offset,
                    is_64bit,
                },
            ) => Self::FileDescriptorWriteV1 {
                data: data.as_ref().into(),
                fd: fd.to_native(),
                offset: offset.to_native(),
                is_64bit: *is_64bit,
            },
            ArchivedJournalEntry::FileDescriptorSeekV1(
                ArchivedJournalEntryFileDescriptorSeekV1 {
                    fd,
                    offset,
                    ref whence,
                },
            ) => Self::FileDescriptorSeekV1 {
                fd: fd.to_native(),
                offset: offset.to_native(),
                whence: whence.into(),
            },
            ArchivedJournalEntry::OpenFileDescriptorV1(
                ArchivedJournalEntryOpenFileDescriptorV1 {
                    fd,
                    dirfd,
                    dirflags,
                    path,
                    o_flags,
                    fs_rights_base,
                    fs_rights_inheriting,
                    fs_flags,
                },
            ) => Self::OpenFileDescriptorV1 {
                fd: fd.to_native(),
                dirfd: dirfd.to_native(),
                dirflags: dirflags.to_native(),
                path: String::from_utf8_lossy(path.as_ref()),
                o_flags: wasi::Oflags::from_bits_truncate(o_flags.to_native()),
                fs_rights_base: wasi::Rights::from_bits_truncate(fs_rights_base.to_native()),
                fs_rights_inheriting: wasi::Rights::from_bits_truncate(
                    fs_rights_inheriting.to_native(),
                ),
                fs_flags: wasi::Fdflags::from_bits_truncate(fs_flags.to_native()),
            },
            ArchivedJournalEntry::OpenFileDescriptorV2(
                ArchivedJournalEntryOpenFileDescriptorV2 {
                    fd,
                    dirfd,
                    dirflags,
                    path,
                    o_flags,
                    fs_rights_base,
                    fs_rights_inheriting,
                    fs_flags,
                    fd_flags,
                },
            ) => Self::OpenFileDescriptorV2 {
                fd: fd.to_native(),
                dirfd: dirfd.to_native(),
                dirflags: dirflags.to_native(),
                path: String::from_utf8_lossy(path.as_ref()),
                o_flags: wasi::Oflags::from_bits_truncate(o_flags.to_native()),
                fs_rights_base: wasi::Rights::from_bits_truncate(fs_rights_base.to_native()),
                fs_rights_inheriting: wasi::Rights::from_bits_truncate(
                    fs_rights_inheriting.to_native(),
                ),
                fs_flags: wasi::Fdflags::from_bits_truncate(fs_flags.to_native()),
                fd_flags: wasi::Fdflagsext::from_bits_truncate(fd_flags.to_native()),
            },
            ArchivedJournalEntry::CloseFileDescriptorV1(
                ArchivedJournalEntryCloseFileDescriptorV1 { fd },
            ) => Self::CloseFileDescriptorV1 { fd: fd.to_native() },
            ArchivedJournalEntry::RemoveDirectoryV1(ArchivedJournalEntryRemoveDirectoryV1 {
                fd,
                path,
            }) => Self::RemoveDirectoryV1 {
                fd: fd.to_native(),
                path: String::from_utf8_lossy(path.as_ref()),
            },
            ArchivedJournalEntry::UnlinkFileV1(ArchivedJournalEntryUnlinkFileV1 { fd, path }) => {
                Self::UnlinkFileV1 {
                    fd: fd.to_native(),
                    path: String::from_utf8_lossy(path.as_ref()),
                }
            }
            ArchivedJournalEntry::PathRenameV1(ArchivedJournalEntryPathRenameV1 {
                old_fd,
                old_path,
                new_fd,
                new_path,
            }) => Self::PathRenameV1 {
                old_fd: old_fd.to_native(),
                old_path: String::from_utf8_lossy(old_path.as_ref()),
                new_fd: new_fd.to_native(),
                new_path: String::from_utf8_lossy(new_path.as_ref()),
            },
            ArchivedJournalEntry::SnapshotV1(ArchivedJournalEntrySnapshotV1 {
                since_epoch,
                ref trigger,
            }) => Self::SnapshotV1 {
                when: SystemTime::UNIX_EPOCH
                    .checked_add((*since_epoch).into())
                    .unwrap_or(SystemTime::UNIX_EPOCH),
                trigger: trigger.into(),
            },
            ArchivedJournalEntry::SetClockTimeV1(ArchivedJournalEntrySetClockTimeV1 {
                ref clock_id,
                time,
            }) => Self::SetClockTimeV1 {
                clock_id: clock_id.into(),
                time: time.to_native(),
            },
            ArchivedJournalEntry::RenumberFileDescriptorV1(
                ArchivedJournalEntryRenumberFileDescriptorV1 { old_fd, new_fd },
            ) => Self::RenumberFileDescriptorV1 {
                old_fd: old_fd.to_native(),
                new_fd: new_fd.to_native(),
            },
            ArchivedJournalEntry::DuplicateFileDescriptorV1(
                ArchivedJournalEntryDuplicateFileDescriptorV1 {
                    original_fd: old_fd,
                    copied_fd: new_fd,
                },
            ) => Self::DuplicateFileDescriptorV1 {
                original_fd: old_fd.to_native(),
                copied_fd: new_fd.to_native(),
            },
            ArchivedJournalEntry::DuplicateFileDescriptorV2(
                ArchivedJournalEntryDuplicateFileDescriptorV2 {
                    original_fd: old_fd,
                    copied_fd: new_fd,
                    cloexec,
                },
            ) => Self::DuplicateFileDescriptorV2 {
                original_fd: old_fd.to_native(),
                copied_fd: new_fd.to_native(),
                cloexec: *cloexec,
            },
            ArchivedJournalEntry::CreateDirectoryV1(ArchivedJournalEntryCreateDirectoryV1 {
                fd,
                path,
            }) => Self::CreateDirectoryV1 {
                fd: fd.to_native(),
                path: String::from_utf8_lossy(path.as_ref()),
            },
            ArchivedJournalEntry::PathSetTimesV1(ArchivedJournalEntryPathSetTimesV1 {
                fd,
                path,
                flags,
                st_atim,
                st_mtim,
                fst_flags,
            }) => Self::PathSetTimesV1 {
                fd: fd.to_native(),
                path: String::from_utf8_lossy(path.as_ref()),
                flags: flags.to_native(),
                st_atim: st_atim.to_native(),
                st_mtim: st_mtim.to_native(),
                fst_flags: wasi::Fstflags::from_bits_truncate(fst_flags.to_native()),
            },
            ArchivedJournalEntry::FileDescriptorSetTimesV1(
                ArchivedJournalEntryFileDescriptorSetTimesV1 {
                    fd,
                    st_atim,
                    st_mtim,
                    fst_flags,
                },
            ) => Self::FileDescriptorSetTimesV1 {
                fd: fd.to_native(),
                st_atim: st_atim.to_native(),
                st_mtim: st_mtim.to_native(),
                fst_flags: wasi::Fstflags::from_bits_truncate(fst_flags.to_native()),
            },
            ArchivedJournalEntry::FileDescriptorSetSizeV1(
                ArchivedJournalEntryFileDescriptorSetSizeV1 { fd, st_size },
            ) => Self::FileDescriptorSetSizeV1 {
                fd: fd.to_native(),
                st_size: st_size.to_native(),
            },
            ArchivedJournalEntry::FileDescriptorSetFdFlagsV1(
                ArchivedJournalEntryFileDescriptorSetFdFlagsV1 { fd, flags },
            ) => Self::FileDescriptorSetFdFlagsV1 {
                fd: fd.to_native(),
                flags: wasi::Fdflagsext::from_bits_truncate(flags.to_native()),
            },
            ArchivedJournalEntry::FileDescriptorSetFlagsV1(
                ArchivedJournalEntryFileDescriptorSetFlagsV1 { fd, flags },
            ) => Self::FileDescriptorSetFlagsV1 {
                fd: fd.to_native(),
                flags: wasi::Fdflags::from_bits_truncate(flags.to_native()),
            },
            ArchivedJournalEntry::FileDescriptorSetRightsV1(
                ArchivedJournalEntryFileDescriptorSetRightsV1 {
                    fd,
                    fs_rights_base,
                    fs_rights_inheriting,
                },
            ) => Self::FileDescriptorSetRightsV1 {
                fd: fd.to_native(),
                fs_rights_base: wasi::Rights::from_bits_truncate(fs_rights_base.to_native()),
                fs_rights_inheriting: wasi::Rights::from_bits_truncate(
                    fs_rights_inheriting.to_native(),
                ),
            },
            ArchivedJournalEntry::FileDescriptorAdviseV1(
                ArchivedJournalEntryFileDescriptorAdviseV1 {
                    fd,
                    offset,
                    len,
                    ref advice,
                },
            ) => Self::FileDescriptorAdviseV1 {
                fd: fd.to_native(),
                offset: offset.to_native(),
                len: len.to_native(),
                advice: advice.into(),
            },
            ArchivedJournalEntry::FileDescriptorAllocateV1(
                ArchivedJournalEntryFileDescriptorAllocateV1 { fd, offset, len },
            ) => Self::FileDescriptorAllocateV1 {
                fd: fd.to_native(),
                offset: offset.to_native(),
                len: len.to_native(),
            },
            ArchivedJournalEntry::CreateHardLinkV1(ArchivedJournalEntryCreateHardLinkV1 {
                old_fd,
                old_path,
                old_flags,
                new_fd,
                new_path,
            }) => Self::CreateHardLinkV1 {
                old_fd: old_fd.to_native(),
                old_path: String::from_utf8_lossy(old_path.as_ref()),
                old_flags: old_flags.to_native(),
                new_fd: new_fd.to_native(),
                new_path: String::from_utf8_lossy(new_path.as_ref()),
            },
            ArchivedJournalEntry::CreateSymbolicLinkV1(
                ArchivedJournalEntryCreateSymbolicLinkV1 {
                    old_path,
                    fd,
                    new_path,
                },
            ) => Self::CreateSymbolicLinkV1 {
                old_path: String::from_utf8_lossy(old_path.as_ref()),
                fd: fd.to_native(),
                new_path: String::from_utf8_lossy(new_path.as_ref()),
            },
            ArchivedJournalEntry::ChangeDirectoryV1(ArchivedJournalEntryChangeDirectoryV1 {
                path,
            }) => Self::ChangeDirectoryV1 {
                path: String::from_utf8_lossy(path.as_ref()),
            },
            ArchivedJournalEntry::EpollCreateV1(ArchivedJournalEntryEpollCreateV1 { fd }) => {
                Self::EpollCreateV1 { fd: fd.to_native() }
            }
            ArchivedJournalEntry::EpollCtlV1(ArchivedJournalEntryEpollCtlV1 {
                epfd,
                ref op,
                fd,
                ref event,
            }) => Self::EpollCtlV1 {
                epfd: epfd.to_native(),
                op: op.into(),
                fd: fd.to_native(),
                event: event.as_ref().map(|e| e.into()),
            },
            ArchivedJournalEntry::TtySetV1(ArchivedJournalEntryTtySetV1 {
                cols,
                rows,
                width,
                height,
                stdin_tty,
                stdout_tty,
                stderr_tty,
                echo,
                line_buffered,
                line_feeds,
            }) => Self::TtySetV1 {
                tty: wasi::Tty {
                    cols: cols.to_native(),
                    rows: rows.to_native(),
                    width: width.to_native(),
                    height: height.to_native(),
                    stdin_tty: *stdin_tty,
                    stdout_tty: *stdout_tty,
                    stderr_tty: *stderr_tty,
                    echo: *echo,
                    line_buffered: *line_buffered,
                },
                line_feeds: *line_feeds,
            },
            ArchivedJournalEntry::CreatePipeV1(ArchivedJournalEntryCreatePipeV1 {
                read_fd,
                write_fd,
            }) => Self::CreatePipeV1 {
                read_fd: read_fd.to_native(),
                write_fd: write_fd.to_native(),
            },
            ArchivedJournalEntry::PortAddAddrV1(ArchivedJournalEntryPortAddAddrV1 { cidr }) => {
                Self::PortAddAddrV1 {
                    cidr: JournalIpCidrV1 {
                        ip: cidr.ip.as_ipaddr(),
                        prefix: cidr.prefix,
                    }
                    .into(),
                }
            }
            ArchivedJournalEntry::PortDelAddrV1(ArchivedJournalEntryPortDelAddrV1 { addr }) => {
                Self::PortDelAddrV1 {
                    addr: addr.as_ipaddr(),
                }
            }
            ArchivedJournalEntry::PortAddrClearV1 => Self::PortAddrClearV1,
            ArchivedJournalEntry::PortBridgeV1(ArchivedJournalEntryPortBridgeV1 {
                network,
                token,
                ref security,
            }) => Self::PortBridgeV1 {
                network: String::from_utf8_lossy(network.as_ref()),
                token: String::from_utf8_lossy(token.as_ref()),
                security: security.into(),
            },
            ArchivedJournalEntry::PortUnbridgeV1 => Self::PortUnbridgeV1,
            ArchivedJournalEntry::PortDhcpAcquireV1 => Self::PortDhcpAcquireV1,
            ArchivedJournalEntry::PortGatewaySetV1(ArchivedJournalEntryPortGatewaySetV1 { ip }) => {
                Self::PortGatewaySetV1 { ip: ip.as_ipaddr() }
            }
            ArchivedJournalEntry::PortRouteAddV1(ArchivedJournalEntryPortRouteAddV1 {
                cidr,
                via_router,
                preferred_until,
                expires_at,
            }) => Self::PortRouteAddV1 {
                cidr: JournalIpCidrV1 {
                    ip: cidr.ip.as_ipaddr(),
                    prefix: cidr.prefix,
                }
                .into(),
                via_router: via_router.as_ipaddr(),
                preferred_until: preferred_until.as_ref().map(|time| (*time).into()),
                expires_at: expires_at.as_ref().map(|time| (*time).into()),
            },
            ArchivedJournalEntry::PortRouteClearV1 => Self::PortRouteClearV1,
            ArchivedJournalEntry::PortRouteDelV1(ArchivedJournalEntryPortRouteDelV1 { ip }) => {
                Self::PortRouteDelV1 { ip: ip.as_ipaddr() }
            }
            ArchivedJournalEntry::SocketOpenV1(ArchivedJournalEntrySocketOpenV1 {
                ref af,
                ref ty,
                pt,
                fd,
            }) => Self::SocketOpenV1 {
                af: af.into(),
                ty: ty.into(),
                pt: (pt.to_native()).try_into().unwrap_or(wasi::SockProto::Max),
                fd: fd.to_native(),
            },
            ArchivedJournalEntry::SocketPairV1(ArchivedJournalEntrySocketPairV1 { fd1, fd2 }) => {
                Self::SocketPairV1 {
                    fd1: fd1.to_native(),
                    fd2: fd2.to_native(),
                }
            }
            ArchivedJournalEntry::SocketListenV1(ArchivedJournalEntrySocketListenV1 {
                fd,
                backlog,
            }) => Self::SocketListenV1 {
                fd: fd.to_native(),
                backlog: backlog.to_native(),
            },
            ArchivedJournalEntry::SocketBindV1(ArchivedJournalEntrySocketBindV1 { fd, addr }) => {
                Self::SocketBindV1 {
                    fd: fd.to_native(),
                    addr: addr.as_socket_addr(),
                }
            }
            ArchivedJournalEntry::SocketConnectedV1(ArchivedJournalEntrySocketConnectedV1 {
                fd,
                local_addr,
                peer_addr,
            }) => Self::SocketConnectedV1 {
                fd: fd.to_native(),
                local_addr: local_addr.as_socket_addr(),
                peer_addr: peer_addr.as_socket_addr(),
            },
            ArchivedJournalEntry::SocketAcceptedV1(ArchivedJournalEntrySocketAcceptedV1 {
                listen_fd,
                fd,
                local_addr,
                peer_addr,
                fd_flags,
                nonblocking,
            }) => Self::SocketAcceptedV1 {
                listen_fd: listen_fd.to_native(),
                fd: fd.to_native(),
                local_addr: local_addr.as_socket_addr(),
                peer_addr: peer_addr.as_socket_addr(),
                fd_flags: wasi::Fdflags::from_bits_truncate(fd_flags.to_native()),
                non_blocking: *nonblocking,
            },
            ArchivedJournalEntry::SocketJoinIpv4MulticastV1(
                ArchivedJournalEntrySocketJoinIpv4MulticastV1 {
                    fd,
                    multiaddr,
                    iface,
                },
            ) => Self::SocketJoinIpv4MulticastV1 {
                fd: fd.to_native(),
                multiaddr: multiaddr.as_ipv4(),
                iface: iface.as_ipv4(),
            },
            ArchivedJournalEntry::SocketJoinIpv6MulticastV1(
                ArchivedJournalEntrySocketJoinIpv6MulticastV1 {
                    fd,
                    multiaddr,
                    iface,
                },
            ) => Self::SocketJoinIpv6MulticastV1 {
                fd: fd.to_native(),
                multi_addr: multiaddr.as_ipv6(),
                iface: iface.to_native(),
            },
            ArchivedJournalEntry::SocketLeaveIpv4MulticastV1(
                ArchivedJournalEntrySocketLeaveIpv4MulticastV1 {
                    fd,
                    multiaddr,
                    iface,
                },
            ) => Self::SocketLeaveIpv4MulticastV1 {
                fd: fd.to_native(),
                multi_addr: multiaddr.as_ipv4(),
                iface: iface.as_ipv4(),
            },
            ArchivedJournalEntry::SocketLeaveIpv6MulticastV1(
                ArchivedJournalEntrySocketLeaveIpv6MulticastV1 {
                    fd,
                    multiaddr,
                    iface,
                },
            ) => Self::SocketLeaveIpv6MulticastV1 {
                fd: fd.to_native(),
                multi_addr: multiaddr.as_ipv6(),
                iface: iface.to_native(),
            },
            ArchivedJournalEntry::SocketSendFileV1(ArchivedJournalEntrySocketSendFileV1 {
                socket_fd,
                file_fd,
                offset,
                count,
            }) => Self::SocketSendFileV1 {
                socket_fd: socket_fd.to_native(),
                file_fd: file_fd.to_native(),
                offset: offset.to_native(),
                count: count.to_native(),
            },
            ArchivedJournalEntry::SocketSendToV1(ArchivedJournalEntrySocketSendToV1 {
                fd,
                data,
                flags,
                addr,
                is_64bit,
            }) => Self::SocketSendToV1 {
                fd: fd.to_native(),
                data: data.as_ref().into(),
                flags: flags.to_native(),
                addr: addr.as_socket_addr(),
                is_64bit: *is_64bit,
            },
            ArchivedJournalEntry::SocketSendV1(ArchivedJournalEntrySocketSendV1 {
                fd,
                data,
                flags,
                is_64bit,
            }) => Self::SocketSendV1 {
                fd: fd.to_native(),
                data: data.as_ref().into(),
                flags: flags.to_native(),
                is_64bit: *is_64bit,
            },
            ArchivedJournalEntry::SocketSetOptFlagV1(ArchivedJournalEntrySocketSetOptFlagV1 {
                fd,
                ref opt,
                flag,
            }) => Self::SocketSetOptFlagV1 {
                fd: fd.to_native(),
                opt: opt.into(),
                flag: *flag,
            },
            ArchivedJournalEntry::SocketSetOptSizeV1(ArchivedJournalEntrySocketSetOptSizeV1 {
                fd,
                ref opt,
                size,
            }) => Self::SocketSetOptSizeV1 {
                fd: fd.to_native(),
                opt: opt.into(),
                size: size.to_native(),
            },
            ArchivedJournalEntry::SocketSetOptTimeV1(ArchivedJournalEntrySocketSetOptTimeV1 {
                fd,
                ref ty,
                time,
            }) => Self::SocketSetOptTimeV1 {
                fd: fd.to_native(),
                ty: ty.into(),
                time: time.as_ref().map(|time| (*time).into()),
            },
            ArchivedJournalEntry::SocketShutdownV1(ArchivedJournalEntrySocketShutdownV1 {
                fd,
                ref how,
            }) => Self::SocketShutdownV1 {
                fd: fd.to_native(),
                how: how.into(),
            },
            ArchivedJournalEntry::CreateEventV1(ArchivedJournalEntryCreateEventV1 {
                initial_val,
                flags,
                fd,
            }) => Self::CreateEventV1 {
                initial_val: initial_val.to_native(),
                flags: flags.to_native(),
                fd: fd.to_native(),
            },
        })
    }
}
