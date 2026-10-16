//! The v1 layouts against `wasmer-wasix-types`, the legacy host's view of the same ABI, and
//! against wasix-libc's static assertions where the two disagree (libc is the guest side, so
//! it wins).

use std::mem::{align_of, offset_of, size_of};

use wasixv2_abi::GuestLayout;
use wasixv2_wasix1::abi::{fs, poll, proc, sock};
use wasmer::Memory32;
use wasmer_wasix_types::wasi as legacy;

/// Asserts that `$v1` has the size and alignment of `$legacy` and each listed field sits at the
/// same offset (`v1_field => legacy_field`).
macro_rules! same_layout {
    ($v1:ty, $legacy:ty, { $($field:ident => $lfield:ident),* $(,)? }) => {{
        assert_eq!(<$v1>::SIZE, size_of::<$legacy>(), "size of {}", stringify!($v1));
        assert_eq!(<$v1>::ALIGN, align_of::<$legacy>(), "align of {}", stringify!($v1));
        $(
            assert_eq!(
                offset(<$v1>::OFFSETS, stringify!($field)),
                offset_of!($legacy, $lfield),
                "{}.{}",
                stringify!($v1),
                stringify!($field),
            );
        )*
    }};
}

fn offset(offsets: &[(&str, usize)], field: &str) -> usize {
    offsets
        .iter()
        .find(|(name, _)| *name == field)
        .unwrap_or_else(|| panic!("no field {field}"))
        .1
}

#[test]
fn fs_layouts() {
    same_layout!(fs::Fdstat, legacy::Fdstat, {
        fs_filetype => fs_filetype,
        fs_flags => fs_flags,
        fs_rights_base => fs_rights_base,
        fs_rights_inheriting => fs_rights_inheriting,
    });
    same_layout!(fs::Filestat, legacy::Filestat, {
        st_dev => st_dev, st_ino => st_ino, st_filetype => st_filetype, st_nlink => st_nlink,
        st_size => st_size, st_atim => st_atim, st_mtim => st_mtim, st_ctim => st_ctim,
    });
    same_layout!(fs::Snapshot0Filestat, legacy::Snapshot0Filestat, {
        st_dev => st_dev, st_ino => st_ino, st_filetype => st_filetype, st_nlink => st_nlink,
        st_size => st_size, st_atim => st_atim, st_mtim => st_mtim, st_ctim => st_ctim,
    });
    same_layout!(fs::Prestat, legacy::Prestat, { pr_type => pr_type });
    assert_eq!(
        offset(fs::Prestat::OFFSETS, "pr_name_len"),
        offset_of!(legacy::Prestat, u)
    );
}

/// The legacy crate's `Dirent` declares `d_type` before `d_namlen` (its host serializes by
/// hand); wasix-libc's `__wasi_dirent_t` has `d_namlen` at 16 and `d_type` at 20.
#[test]
fn dirent_matches_libc() {
    assert_eq!(fs::Dirent::SIZE, size_of::<legacy::Dirent>());
    assert_eq!(fs::Dirent::SIZE, 24);
    assert_eq!(offset(fs::Dirent::OFFSETS, "d_next"), 0);
    assert_eq!(offset(fs::Dirent::OFFSETS, "d_ino"), 8);
    assert_eq!(offset(fs::Dirent::OFFSETS, "d_namlen"), 16);
    assert_eq!(offset(fs::Dirent::OFFSETS, "d_type"), 20);
}

#[test]
fn poll_layouts() {
    same_layout!(poll::SubscriptionClock, legacy::SubscriptionClock, {
        id => clock_id, timeout => timeout, precision => precision, flags => flags,
    });
    same_layout!(poll::Snapshot0SubscriptionClock, legacy::Snapshot0SubscriptionClock, {
        identifier => identifier, id => id, timeout => timeout, precision => precision,
        flags => flags,
    });
    same_layout!(poll::Subscription, legacy::Subscription, {
        userdata => userdata, tag => type_,
    });
    assert_eq!(
        offset(poll::Subscription::OFFSETS, "payload"),
        offset_of!(legacy::Subscription, data)
    );
    same_layout!(poll::Snapshot0Subscription, legacy::Snapshot0Subscription, {
        userdata => userdata, tag => type_,
    });
    assert_eq!(
        offset(poll::Snapshot0Subscription::OFFSETS, "payload"),
        offset_of!(legacy::Snapshot0Subscription, u)
    );
    same_layout!(poll::EventFdReadwrite, legacy::EventFdReadwrite, {
        nbytes => nbytes, flags => flags,
    });
    same_layout!(poll::Event, legacy::Event, {
        userdata => userdata, error => error, type_ => type_,
    });
    assert_eq!(
        offset(poll::Event::OFFSETS, "fd_readwrite"),
        offset_of!(legacy::Event, u)
    );
}

#[test]
fn proc_layouts() {
    same_layout!(proc::OptionPid, legacy::OptionPid, { tag => tag, pid => pid });
    same_layout!(proc::OptionTimestamp, legacy::OptionTimestamp, {
        tag => tag, timestamp => u,
    });
    same_layout!(proc::SignalDisposition, legacy::SignalDisposition, {
        sig => sig, disp => disp,
    });
    same_layout!(proc::ThreadStart, legacy::ThreadStart<Memory32>, {
        stack => stack_upper, tls_base => tls_base, start_funct => start_funct,
        start_args => start_args, stack_size => stack_size, guard_size => guard_size,
    });
    same_layout!(proc::ProcSpawnFdOp, legacy::ProcSpawnFdOp<Memory32>, {
        cmd => cmd, fd => fd, src_fd => src_fd, path => name, path_len => name_len,
        dirflags => dirflags, oflags => oflags, fs_rights_base => fs_rights_base,
        fs_rights_inheriting => fs_rights_inheriting, fdflags => fdflags,
        fdflagsext => fdflagsext,
    });
    same_layout!(proc::StackSnapshot, legacy::StackSnapshot, { user => user });
    assert_eq!(proc::JoinStatus::SIZE, size_of::<legacy::JoinStatus>());
    // `tty_t` grew `line_feeds` in wasix-libc after the legacy crate's `Tty`.
    assert_eq!(proc::Tty::SIZE, 24);
    assert_eq!(offset(proc::Tty::OFFSETS, "line_feeds"), 21);
}

#[test]
fn sock_layouts() {
    same_layout!(sock::EpollData, legacy::EpollData<Memory32>, {
        ptr => ptr, fd => fd, data1 => data1, data2 => data2,
    });
    same_layout!(sock::EpollEvent, legacy::EpollEvent<Memory32>, {
        events => events, data => data,
    });
    // wasix-libc's `__wasi_addr_port_t` and `__wasi_addr_t` hold the unix variant (108 bytes
    // of path); the legacy host only declared the inet ones.
    assert_eq!(sock::AddrPort::SIZE, 110);
    assert_eq!(sock::Addr::SIZE, 110);
    assert_eq!(sock::AddrCidr::SIZE, 28);
}
