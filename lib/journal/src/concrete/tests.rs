use std::{
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    time::{Duration, SystemTime},
};

use super::*;
use lz4_flex::compress_prepend_size;
use rkyv::{
    api::high::HighSerializer,
    rancor::Strategy,
    ser::{
        allocator::{Arena, ArenaHandle},
        sharing::Share,
        Serializer,
    },
};
use wasmer_wasix_types::wasi;

pub fn run_test(record: JournalEntry<'_>) {
    tracing::info!("record: {:?}", record);

    // Determine the record type
    let record_type = record.archive_record_type();
    tracing::info!("record_type: {:?}", record_type);

    // Serialize it
    let mut arena = Arena::new();
    let mut buffer = Vec::new();
    let mut serializer = Serializer::new(&mut buffer, arena.acquire(), Share::new());
    let serializer: &mut HighSerializer<&mut Vec<u8>, ArenaHandle, rkyv::rancor::Error> =
        Strategy::wrap(&mut serializer);

    record.clone().serialize_archive(serializer).unwrap();
    let buffer = &buffer[..];
    if buffer.len() < 20 {
        tracing::info!("buffer: {:x?}", buffer);
    } else {
        tracing::info!("buffer_len: {}", buffer.len());
    }

    // Deserialize it
    let record2 = unsafe { record_type.deserialize_archive(buffer).unwrap() };
    tracing::info!("record2: {:?}", record2);

    // Check it
    assert_eq!(record, record2);

    // Now make it static and check it again
    let record3 = record2.into_owned();
    tracing::info!("record3: {:?}", record3);
    assert_eq!(record, record3);
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_init_module() {
    run_test(JournalEntry::InitModuleV1 {
        wasm_hash: Box::new([13u8; 8]),
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_process_exit() {
    run_test(JournalEntry::ProcessExitV1 {
        exit_code: Some(wasi::ExitCode::from(wasi::Errno::Fault)),
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_set_thread() {
    run_test(JournalEntry::SetThreadV1 {
        id: 1234u32,
        call_stack: vec![1, 2, 3].into(),
        memory_stack: vec![4, 5, 6, 7].into(),
        store_data: vec![10, 11].into(),
        is_64bit: true,
        layout: wasmer_wasix_types::wasix::WasiMemoryLayout {
            stack_upper: 0,
            stack_lower: 1024,
            guard_size: 16,
            stack_size: 1024,
        },
        start: wasmer_wasix_types::wasix::ThreadStartType::MainThread,
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_close_thread() {
    run_test(JournalEntry::CloseThreadV1 {
        id: 987u32,
        exit_code: Some(wasi::ExitCode::from(wasi::Errno::Fault)),
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_descriptor_seek() {
    run_test(JournalEntry::FileDescriptorSeekV1 {
        fd: 765u32,
        offset: 9183722450971234i64,
        whence: wasi::Whence::End,
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_descriptor_write() {
    run_test(JournalEntry::FileDescriptorWriteV1 {
        fd: 54321u32,
        offset: 13897412934u64,
        data: vec![74u8, 98u8, 36u8].into(),
        is_64bit: true,
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_update_memory() {
    run_test(JournalEntry::UpdateMemoryRegionV1 {
        region: 76u64..8237453u64,
        compressed_data: compress_prepend_size(&[74u8; 40960]).into(),
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_set_clock_time() {
    run_test(JournalEntry::SetClockTimeV1 {
        clock_id: wasi::Snapshot0Clockid::Realtime,
        time: 7912837412934u64,
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_open_file_descriptor() {
    run_test(JournalEntry::OpenFileDescriptorV1 {
        fd: 298745u32,
        dirfd: 23458922u32,
        dirflags: 134512345,
        path: "/blah".into(),
        o_flags: wasi::Oflags::all(),
        fs_rights_base: wasi::Rights::all(),
        fs_rights_inheriting: wasi::Rights::all(),
        fs_flags: wasi::Fdflags::all(),
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_close_descriptor() {
    run_test(JournalEntry::CloseFileDescriptorV1 { fd: 23845732u32 });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_renumber_file_descriptor() {
    run_test(JournalEntry::RenumberFileDescriptorV1 {
        old_fd: 27834u32,
        new_fd: 398452345u32,
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_duplicate_file_descriptor() {
    run_test(JournalEntry::DuplicateFileDescriptorV1 {
        original_fd: 23482934u32,
        copied_fd: 9384529u32,
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_create_directory() {
    run_test(JournalEntry::CreateDirectoryV1 {
        fd: 238472u32,
        path: "/joasjdf/asdfn".into(),
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_remove_directory() {
    run_test(JournalEntry::RemoveDirectoryV1 {
        fd: 23894952u32,
        path: "/blahblah".into(),
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_path_set_times() {
    run_test(JournalEntry::PathSetTimesV1 {
        fd: 1238934u32,
        flags: 234523,
        path: "/".into(),
        st_atim: 923452345,
        st_mtim: 350,
        fst_flags: wasi::Fstflags::all(),
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_file_descriptor_set_times() {
    run_test(JournalEntry::FileDescriptorSetTimesV1 {
        fd: 898785u32,
        st_atim: 29834952345,
        st_mtim: 239845892345,
        fst_flags: wasi::Fstflags::all(),
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_file_descriptor_set_size() {
    run_test(JournalEntry::FileDescriptorSetSizeV1 {
        fd: 34958234u32,
        st_size: 234958293845u64,
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_file_descriptor_set_flags() {
    run_test(JournalEntry::FileDescriptorSetFlagsV1 {
        fd: 982348752u32,
        flags: wasi::Fdflags::all(),
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_file_descriptor_set_rights() {
    run_test(JournalEntry::FileDescriptorSetRightsV1 {
        fd: 872345u32,
        fs_rights_base: wasi::Rights::all(),
        fs_rights_inheriting: wasi::Rights::all(),
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_file_descriptor_advise() {
    run_test(JournalEntry::FileDescriptorAdviseV1 {
        fd: 298434u32,
        offset: 92834529092345,
        len: 23485928345,
        advice: wasi::Advice::Random,
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_file_descriptor_allocate() {
    run_test(JournalEntry::FileDescriptorAllocateV1 {
        fd: 2934852,
        offset: 23489582934523,
        len: 9845982345,
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_create_hard_link() {
    run_test(JournalEntry::CreateHardLinkV1 {
        old_fd: 324983845,
        old_path: "/asjdfiasidfasdf".into(),
        old_flags: 234857,
        new_fd: 34958345,
        new_path: "/ashdufnasd".into(),
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_create_symbolic_link() {
    run_test(JournalEntry::CreateSymbolicLinkV1 {
        old_path: "/asjbndfjasdf/asdafasdf".into(),
        fd: 235422345,
        new_path: "/asdf".into(),
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_unlink_file() {
    run_test(JournalEntry::UnlinkFileV1 {
        fd: 32452345,
        path: "/asdfasd".into(),
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_path_rename() {
    run_test(JournalEntry::PathRenameV1 {
        old_fd: 32451345,
        old_path: "/asdfasdfas/asdfasdf".into(),
        new_fd: 23452345,
        new_path: "/ahgfdfghdfghdfgh".into(),
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_change_directory() {
    run_test(JournalEntry::ChangeDirectoryV1 {
        path: "/etc".to_string().into(),
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_epoll_create() {
    run_test(JournalEntry::EpollCreateV1 { fd: 45384752 });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_epoll_ctl() {
    run_test(JournalEntry::EpollCtlV1 {
        epfd: 34523455,
        op: wasi::EpollCtl::Unknown,
        fd: 23452345,
        event: Some(wasi::EpollEventCtl {
            events: wasi::EpollType::all(),
            ptr: 32452345,
            fd: 23452345,
            data1: 1235245756,
            data2: 23452345,
        }),
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_tty_set() {
    run_test(JournalEntry::TtySetV1 {
        tty: wasi::Tty {
            cols: 1234,
            rows: 6754,
            width: 4563456,
            height: 345,
            stdin_tty: true,
            stdout_tty: false,
            stderr_tty: true,
            echo: true,
            line_buffered: true,
        },
        line_feeds: true,
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_create_pipe() {
    run_test(JournalEntry::CreatePipeV1 {
        read_fd: 3452345,
        write_fd: 2345163,
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_create_event() {
    run_test(JournalEntry::CreateEventV1 {
        initial_val: 13451345,
        flags: 2343,
        fd: 5836544,
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_port_add_addr() {
    run_test(JournalEntry::PortAddAddrV1 {
        cidr: JournalIpCidrV1 {
            ip: Ipv4Addr::LOCALHOST.into(),
            prefix: 24,
        }
        .into(),
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_del_addr() {
    run_test(JournalEntry::PortDelAddrV1 {
        addr: Ipv6Addr::LOCALHOST.into(),
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_addr_clear() {
    run_test(JournalEntry::PortAddrClearV1);
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_port_bridge() {
    run_test(JournalEntry::PortBridgeV1 {
        network: "mynetwork".into(),
        token: "blh blah".to_string().into(),
        security: JournalStreamSecurityV1::ClassicEncryption.into(),
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_unbridge() {
    run_test(JournalEntry::PortUnbridgeV1);
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_dhcp_acquire() {
    run_test(JournalEntry::PortDhcpAcquireV1);
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_gateway_set() {
    run_test(JournalEntry::PortGatewaySetV1 {
        ip: Ipv4Addr::new(12, 34, 136, 220).into(),
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_route_add() {
    run_test(JournalEntry::PortRouteAddV1 {
        cidr: JournalIpCidrV1 {
            ip: Ipv4Addr::LOCALHOST.into(),
            prefix: 24,
        }
        .into(),
        via_router: Ipv4Addr::LOCALHOST.into(),
        preferred_until: Some(Duration::MAX),
        expires_at: Some(Duration::ZERO),
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_route_clear() {
    run_test(JournalEntry::PortRouteClearV1);
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_route_del() {
    run_test(JournalEntry::PortRouteDelV1 {
        ip: Ipv4Addr::BROADCAST.into(),
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_socket_open() {
    run_test(JournalEntry::SocketOpenV1 {
        af: wasi::Addressfamily::Inet6,
        ty: wasi::Socktype::Stream,
        pt: wasi::SockProto::Tcp,
        fd: 23452345,
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_socket_listen() {
    run_test(JournalEntry::SocketListenV1 {
        fd: 12341234,
        backlog: 123,
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_socket_bind() {
    run_test(JournalEntry::SocketBindV1 {
        fd: 2341234,
        addr: SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), 1234),
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_socket_connected() {
    run_test(JournalEntry::SocketConnectedV1 {
        fd: 12341,
        local_addr: SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), 1234),
        peer_addr: SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), 1234),
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_socket_accepted() {
    run_test(JournalEntry::SocketAcceptedV1 {
        listen_fd: 21234,
        fd: 1,
        local_addr: SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), 3452),
        peer_addr: SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), 3452),
        fd_flags: wasi::Fdflags::all(),
        non_blocking: true,
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_socket_join_ipv4_multicast() {
    run_test(JournalEntry::SocketJoinIpv4MulticastV1 {
        fd: 12,
        multiaddr: Ipv4Addr::new(123, 123, 123, 123),
        iface: Ipv4Addr::new(128, 0, 0, 1),
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_socket_join_ipv6_multicast() {
    run_test(JournalEntry::SocketJoinIpv6MulticastV1 {
        fd: 12,
        multi_addr: Ipv6Addr::new(123, 123, 123, 123, 1234, 12663, 31, 1324),
        iface: 23541,
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_socket_leave_ipv4_multicast() {
    run_test(JournalEntry::SocketLeaveIpv4MulticastV1 {
        fd: 12,
        multi_addr: Ipv4Addr::new(123, 123, 123, 123),
        iface: Ipv4Addr::new(128, 0, 0, 1),
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_socket_leave_ipv6_multicast() {
    run_test(JournalEntry::SocketLeaveIpv6MulticastV1 {
        fd: 12,
        multi_addr: Ipv6Addr::new(123, 123, 123, 123, 1234, 12663, 31, 1324),
        iface: 23541,
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_socket_send_file() {
    run_test(JournalEntry::SocketSendFileV1 {
        socket_fd: 22234,
        file_fd: 989,
        offset: 124,
        count: 345673456234651234,
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_socket_send_to() {
    run_test(JournalEntry::SocketSendToV1 {
        fd: 123,
        data: [98u8; 102400].to_vec().into(),
        flags: 1234,
        addr: SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), 3452),
        is_64bit: true,
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_socket_send() {
    run_test(JournalEntry::SocketSendV1 {
        fd: 123,
        data: [98u8; 102400].to_vec().into(),
        flags: 1234,
        is_64bit: true,
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_socket_set_opt_flag() {
    run_test(JournalEntry::SocketSetOptFlagV1 {
        fd: 0,
        opt: wasi::Sockoption::Linger,
        flag: true,
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_socket_set_opt_size() {
    run_test(JournalEntry::SocketSetOptSizeV1 {
        fd: 15,
        opt: wasi::Sockoption::Linger,
        size: 234234,
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_socket_set_opt_time() {
    run_test(JournalEntry::SocketSetOptTimeV1 {
        fd: 0,
        ty: SocketOptTimeType::AcceptTimeout,
        time: Some(Duration::ZERO),
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_socket_shutdown() {
    run_test(JournalEntry::SocketShutdownV1 {
        fd: 123,
        how: SocketShutdownHow::Both,
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_snapshot() {
    run_test(JournalEntry::SnapshotV1 {
        when: SystemTime::now(),
        trigger: SnapshotTrigger::Idle,
    });
}

#[tracing_test::traced_test]
#[test]
pub fn test_record_alignment() {
    assert_eq!(std::mem::align_of::<JournalEntryInitModuleV1>(), 8);
    assert_eq!(std::mem::align_of::<JournalEntryProcessExitV1>(), 8);
    assert_eq!(std::mem::align_of::<JournalEntrySetThreadV1>(), 8);
    assert_eq!(std::mem::align_of::<JournalEntryCloseThreadV1>(), 8);
    assert_eq!(std::mem::align_of::<JournalEntryFileDescriptorSeekV1>(), 8);
    assert_eq!(std::mem::align_of::<JournalEntryFileDescriptorWriteV1>(), 8);
    assert_eq!(std::mem::align_of::<JournalEntryUpdateMemoryRegionV1>(), 8);
    assert_eq!(std::mem::align_of::<JournalEntrySetClockTimeV1>(), 8);
    assert_eq!(std::mem::align_of::<JournalEntryOpenFileDescriptorV1>(), 8);
    assert_eq!(std::mem::align_of::<JournalEntryCloseFileDescriptorV1>(), 8);
    assert_eq!(
        std::mem::align_of::<JournalEntryRenumberFileDescriptorV1>(),
        8
    );
    assert_eq!(
        std::mem::align_of::<JournalEntryDuplicateFileDescriptorV1>(),
        8
    );
    assert_eq!(std::mem::align_of::<JournalEntryCreateDirectoryV1>(), 8);
    assert_eq!(std::mem::align_of::<JournalEntryRemoveDirectoryV1>(), 8);
    assert_eq!(std::mem::align_of::<JournalEntryPathSetTimesV1>(), 8);
    assert_eq!(
        std::mem::align_of::<JournalEntryFileDescriptorSetTimesV1>(),
        8
    );
    assert_eq!(
        std::mem::align_of::<JournalEntryFileDescriptorSetSizeV1>(),
        8
    );
    assert_eq!(
        std::mem::align_of::<JournalEntryFileDescriptorSetFlagsV1>(),
        8
    );
    assert_eq!(
        std::mem::align_of::<JournalEntryFileDescriptorSetRightsV1>(),
        8
    );
    assert_eq!(
        std::mem::align_of::<JournalEntryFileDescriptorAdviseV1>(),
        8
    );
    assert_eq!(
        std::mem::align_of::<JournalEntryFileDescriptorAllocateV1>(),
        8
    );
    assert_eq!(std::mem::align_of::<JournalEntryCreateHardLinkV1>(), 8);
    assert_eq!(std::mem::align_of::<JournalEntryCreateSymbolicLinkV1>(), 8);
    assert_eq!(std::mem::align_of::<JournalEntryUnlinkFileV1>(), 8);
    assert_eq!(std::mem::align_of::<JournalEntryPathRenameV1>(), 8);
    assert_eq!(std::mem::align_of::<JournalEntryChangeDirectoryV1>(), 8);
    assert_eq!(std::mem::align_of::<JournalEntryEpollCreateV1>(), 8);
    assert_eq!(std::mem::align_of::<JournalEntryEpollCtlV1>(), 8);
    assert_eq!(std::mem::align_of::<JournalEntryTtySetV1>(), 8);
    assert_eq!(std::mem::align_of::<JournalEntryCreatePipeV1>(), 8);
    assert_eq!(std::mem::align_of::<JournalEntryCreateEventV1>(), 8);
    assert_eq!(std::mem::align_of::<JournalEntryPortAddAddrV1>(), 8);
    assert_eq!(std::mem::align_of::<JournalEntryPortDelAddrV1>(), 8);
    assert_eq!(std::mem::align_of::<JournalEntryPortBridgeV1>(), 8);
    assert_eq!(std::mem::align_of::<JournalEntryPortGatewaySetV1>(), 8);
    assert_eq!(std::mem::align_of::<JournalEntryPortRouteAddV1>(), 8);
    assert_eq!(std::mem::align_of::<JournalEntryPortRouteDelV1>(), 8);
    assert_eq!(std::mem::align_of::<JournalEntrySocketOpenV1>(), 8);
    assert_eq!(std::mem::align_of::<JournalEntrySocketListenV1>(), 8);
    assert_eq!(std::mem::align_of::<JournalEntrySocketBindV1>(), 8);
    assert_eq!(std::mem::align_of::<JournalEntrySocketConnectedV1>(), 8);
    assert_eq!(std::mem::align_of::<JournalEntrySocketAcceptedV1>(), 8);
    assert_eq!(
        std::mem::align_of::<JournalEntrySocketJoinIpv4MulticastV1>(),
        8
    );
    assert_eq!(
        std::mem::align_of::<JournalEntrySocketJoinIpv6MulticastV1>(),
        8
    );
    assert_eq!(
        std::mem::align_of::<JournalEntrySocketLeaveIpv4MulticastV1>(),
        8
    );
    assert_eq!(
        std::mem::align_of::<JournalEntrySocketLeaveIpv6MulticastV1>(),
        8
    );
    assert_eq!(std::mem::align_of::<JournalEntrySocketSendFileV1>(), 8);
    assert_eq!(std::mem::align_of::<JournalEntrySocketSendToV1>(), 8);
    assert_eq!(std::mem::align_of::<JournalEntrySocketSendV1>(), 8);
    assert_eq!(std::mem::align_of::<JournalEntrySocketSetOptFlagV1>(), 8);
    assert_eq!(std::mem::align_of::<JournalEntrySocketSetOptSizeV1>(), 8);
    assert_eq!(std::mem::align_of::<JournalEntrySocketSetOptTimeV1>(), 8);
    assert_eq!(std::mem::align_of::<JournalEntrySocketShutdownV1>(), 8);
    assert_eq!(std::mem::align_of::<JournalEntrySnapshotV1>(), 8);
}
