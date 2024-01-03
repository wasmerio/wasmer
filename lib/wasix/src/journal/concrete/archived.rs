use lz4_flex::block::{compress_prepend_size, decompress_size_prepended};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use rkyv::ser::{ScratchSpace, Serializer};
use rkyv::{Archive, CheckBytes, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use std::borrow::Cow;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::{Duration, SystemTime};
use virtual_net::{IpCidr, StreamSecurity};
use wasmer_wasix_types::wasi::{self, EpollEventCtl, EpollType, Fdflags, Rights, Sockoption};

use super::*;

pub const JOURNAL_MAGIC_NUMBER: u64 = 0x310d6dd027362979;
pub const JOURNAL_MAGIC_NUMBER_BYTES: [u8; 8] = JOURNAL_MAGIC_NUMBER.to_be_bytes();

#[repr(u16)]
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    IntoPrimitive,
    TryFromPrimitive,
    RkyvSerialize,
    RkyvDeserialize,
    Archive,
)]
#[archive_attr(derive(CheckBytes))]
pub enum JournalEntryRecordType {
    InitModuleV1 = 1,
    ProcessExitV1 = 2,
    SetThreadV1 = 3,
    CloseThreadV1 = 4,
    FileDescriptorSeekV1 = 5,
    FileDescriptorWriteV1 = 6,
    UpdateMemoryRegionV1 = 7,
    SetClockTimeV1 = 9,
    OpenFileDescriptorV1 = 10,
    CloseFileDescriptorV1 = 11,
    RenumberFileDescriptorV1 = 12,
    DuplicateFileDescriptorV1 = 13,
    CreateDirectoryV1 = 14,
    RemoveDirectoryV1 = 15,
    PathSetTimesV1 = 16,
    FileDescriptorSetTimesV1 = 17,
    FileDescriptorSetSizeV1 = 18,
    FileDescriptorSetFlagsV1 = 19,
    FileDescriptorSetRightsV1 = 20,
    FileDescriptorAdviseV1 = 21,
    FileDescriptorAllocateV1 = 22,
    CreateHardLinkV1 = 23,
    CreateSymbolicLinkV1 = 24,
    UnlinkFileV1 = 25,
    PathRenameV1 = 26,
    ChangeDirectoryV1 = 27,
    EpollCreateV1 = 28,
    EpollCtlV1 = 29,
    TtySetV1 = 30,
    CreatePipeV1 = 31,
    CreateEventV1 = 32,
    PortAddAddrV1 = 33,
    PortDelAddrV1 = 34,
    PortAddrClearV1 = 35,
    PortBridgeV1 = 36,
    PortUnbridgeV1 = 37,
    PortDhcpAcquireV1 = 38,
    PortGatewaySetV1 = 39,
    PortRouteAddV1 = 40,
    PortRouteClearV1 = 41,
    PortRouteDelV1 = 42,
    SocketOpenV1 = 43,
    SocketListenV1 = 44,
    SocketBindV1 = 45,
    SocketConnectedV1 = 46,
    SocketAcceptedV1 = 47,
    SocketJoinIpv4MulticastV1 = 48,
    SocketJoinIpv6MulticastV1 = 49,
    SocketLeaveIpv4MulticastV1 = 50,
    SocketLeaveIpv6MulticastV1 = 51,
    SocketSendFileV1 = 52,
    SocketSendToV1 = 53,
    SocketSendV1 = 54,
    SocketSetOptFlagV1 = 55,
    SocketSetOptSizeV1 = 56,
    SocketSetOptTimeV1 = 57,
    SocketShutdownV1 = 58,
    SnapshotV1 = 59,
}

impl JournalEntryRecordType {
    /// # Safety
    ///
    /// `rykv` makes direct memory references to achieve high performance
    /// however this does mean care must be taken that the data itself
    /// can not be manipulated or corrupted.
    pub unsafe fn deserialize_archive(self, data: &[u8]) -> anyhow::Result<JournalEntry<'_>> {
        match self {
            JournalEntryRecordType::InitModuleV1 => ArchivedJournalEntry::InitModuleV1(
                rkyv::archived_root::<JournalEntryInitModuleV1>(data),
            ),
            JournalEntryRecordType::ProcessExitV1 => ArchivedJournalEntry::ProcessExitV1(
                rkyv::archived_root::<JournalEntryProcessExitV1>(data),
            ),
            JournalEntryRecordType::SetThreadV1 => ArchivedJournalEntry::SetThreadV1(
                rkyv::archived_root::<JournalEntrySetThreadV1>(data),
            ),
            JournalEntryRecordType::CloseThreadV1 => ArchivedJournalEntry::CloseThreadV1(
                rkyv::archived_root::<JournalEntryCloseThreadV1>(data),
            ),
            JournalEntryRecordType::FileDescriptorSeekV1 => {
                ArchivedJournalEntry::FileDescriptorSeekV1(rkyv::archived_root::<
                    JournalEntryFileDescriptorSeekV1,
                >(data))
            }
            JournalEntryRecordType::FileDescriptorWriteV1 => {
                ArchivedJournalEntry::FileDescriptorWriteV1(rkyv::archived_root::<
                    JournalEntryFileDescriptorWriteV1,
                >(data))
            }
            JournalEntryRecordType::UpdateMemoryRegionV1 => {
                ArchivedJournalEntry::UpdateMemoryRegionV1(rkyv::archived_root::<
                    JournalEntryUpdateMemoryRegionV1,
                >(data))
            }
            JournalEntryRecordType::SetClockTimeV1 => {
                ArchivedJournalEntry::SetClockTimeV1(rkyv::archived_root::<
                    JournalEntrySetClockTimeV1,
                >(data))
            }
            JournalEntryRecordType::OpenFileDescriptorV1 => {
                ArchivedJournalEntry::OpenFileDescriptorV1(rkyv::archived_root::<
                    JournalEntryOpenFileDescriptorV1,
                >(data))
            }
            JournalEntryRecordType::CloseFileDescriptorV1 => {
                ArchivedJournalEntry::CloseFileDescriptorV1(rkyv::archived_root::<
                    JournalEntryCloseFileDescriptorV1,
                >(data))
            }
            JournalEntryRecordType::RenumberFileDescriptorV1 => {
                ArchivedJournalEntry::RenumberFileDescriptorV1(rkyv::archived_root::<
                    JournalEntryRenumberFileDescriptorV1,
                >(data))
            }
            JournalEntryRecordType::DuplicateFileDescriptorV1 => {
                ArchivedJournalEntry::DuplicateFileDescriptorV1(rkyv::archived_root::<
                    JournalEntryDuplicateFileDescriptorV1,
                >(data))
            }
            JournalEntryRecordType::CreateDirectoryV1 => {
                ArchivedJournalEntry::CreateDirectoryV1(rkyv::archived_root::<
                    JournalEntryCreateDirectoryV1,
                >(data))
            }
            JournalEntryRecordType::RemoveDirectoryV1 => {
                ArchivedJournalEntry::RemoveDirectoryV1(rkyv::archived_root::<
                    JournalEntryRemoveDirectoryV1,
                >(data))
            }
            JournalEntryRecordType::PathSetTimesV1 => {
                ArchivedJournalEntry::PathSetTimesV1(rkyv::archived_root::<
                    JournalEntryPathSetTimesV1,
                >(data))
            }
            JournalEntryRecordType::FileDescriptorSetTimesV1 => {
                ArchivedJournalEntry::FileDescriptorSetTimesV1(rkyv::archived_root::<
                    JournalEntryFileDescriptorSetTimesV1,
                >(data))
            }
            JournalEntryRecordType::FileDescriptorSetSizeV1 => {
                ArchivedJournalEntry::FileDescriptorSetSizeV1(rkyv::archived_root::<
                    JournalEntryFileDescriptorSetSizeV1,
                >(data))
            }
            JournalEntryRecordType::FileDescriptorSetFlagsV1 => {
                ArchivedJournalEntry::FileDescriptorSetFlagsV1(rkyv::archived_root::<
                    JournalEntryFileDescriptorSetFlagsV1,
                >(data))
            }
            JournalEntryRecordType::FileDescriptorSetRightsV1 => {
                ArchivedJournalEntry::FileDescriptorSetRightsV1(rkyv::archived_root::<
                    JournalEntryFileDescriptorSetRightsV1,
                >(data))
            }
            JournalEntryRecordType::FileDescriptorAdviseV1 => {
                ArchivedJournalEntry::FileDescriptorAdviseV1(rkyv::archived_root::<
                    JournalEntryFileDescriptorAdviseV1,
                >(data))
            }
            JournalEntryRecordType::FileDescriptorAllocateV1 => {
                ArchivedJournalEntry::FileDescriptorAllocateV1(rkyv::archived_root::<
                    JournalEntryFileDescriptorAllocateV1,
                >(data))
            }
            JournalEntryRecordType::CreateHardLinkV1 => {
                ArchivedJournalEntry::CreateHardLinkV1(rkyv::archived_root::<
                    JournalEntryCreateHardLinkV1,
                >(data))
            }
            JournalEntryRecordType::CreateSymbolicLinkV1 => {
                ArchivedJournalEntry::CreateSymbolicLinkV1(rkyv::archived_root::<
                    JournalEntryCreateSymbolicLinkV1,
                >(data))
            }
            JournalEntryRecordType::UnlinkFileV1 => ArchivedJournalEntry::UnlinkFileV1(
                rkyv::archived_root::<JournalEntryUnlinkFileV1>(data),
            ),
            JournalEntryRecordType::PathRenameV1 => ArchivedJournalEntry::PathRenameV1(
                rkyv::archived_root::<JournalEntryPathRenameV1>(data),
            ),
            JournalEntryRecordType::ChangeDirectoryV1 => {
                ArchivedJournalEntry::ChangeDirectoryV1(rkyv::archived_root::<
                    JournalEntryChangeDirectoryV1,
                >(data))
            }
            JournalEntryRecordType::EpollCreateV1 => ArchivedJournalEntry::EpollCreateV1(
                rkyv::archived_root::<JournalEntryEpollCreateV1>(data),
            ),
            JournalEntryRecordType::EpollCtlV1 => ArchivedJournalEntry::EpollCtlV1(
                rkyv::archived_root::<JournalEntryEpollCtlV1>(data),
            ),
            JournalEntryRecordType::TtySetV1 => {
                ArchivedJournalEntry::TtySetV1(rkyv::archived_root::<JournalEntryTtySetV1>(data))
            }
            JournalEntryRecordType::CreatePipeV1 => ArchivedJournalEntry::CreatePipeV1(
                rkyv::archived_root::<JournalEntryCreatePipeV1>(data),
            ),
            JournalEntryRecordType::CreateEventV1 => ArchivedJournalEntry::CreateEventV1(
                rkyv::archived_root::<JournalEntryCreateEventV1>(data),
            ),
            JournalEntryRecordType::PortAddAddrV1 => ArchivedJournalEntry::PortAddAddrV1(
                rkyv::archived_root::<JournalEntryPortAddAddrV1>(data),
            ),
            JournalEntryRecordType::PortDelAddrV1 => ArchivedJournalEntry::PortDelAddrV1(
                rkyv::archived_root::<JournalEntryPortDelAddrV1>(data),
            ),
            JournalEntryRecordType::PortAddrClearV1 => return Ok(JournalEntry::PortAddrClearV1),
            JournalEntryRecordType::PortBridgeV1 => ArchivedJournalEntry::PortBridgeV1(
                rkyv::archived_root::<JournalEntryPortBridgeV1>(data),
            ),
            JournalEntryRecordType::PortUnbridgeV1 => return Ok(JournalEntry::PortUnbridgeV1),
            JournalEntryRecordType::PortDhcpAcquireV1 => {
                return Ok(JournalEntry::PortDhcpAcquireV1)
            }
            JournalEntryRecordType::PortGatewaySetV1 => {
                ArchivedJournalEntry::PortGatewaySetV1(rkyv::archived_root::<
                    JournalEntryPortGatewaySetV1,
                >(data))
            }
            JournalEntryRecordType::PortRouteAddV1 => {
                ArchivedJournalEntry::PortRouteAddV1(rkyv::archived_root::<
                    JournalEntryPortRouteAddV1,
                >(data))
            }
            JournalEntryRecordType::PortRouteClearV1 => return Ok(JournalEntry::PortRouteClearV1),
            JournalEntryRecordType::PortRouteDelV1 => {
                ArchivedJournalEntry::PortRouteDelV1(rkyv::archived_root::<
                    JournalEntryPortRouteDelV1,
                >(data))
            }
            JournalEntryRecordType::SocketOpenV1 => ArchivedJournalEntry::SocketOpenV1(
                rkyv::archived_root::<JournalEntrySocketOpenV1>(data),
            ),
            JournalEntryRecordType::SocketListenV1 => {
                ArchivedJournalEntry::SocketListenV1(rkyv::archived_root::<
                    JournalEntrySocketListenV1,
                >(data))
            }
            JournalEntryRecordType::SocketBindV1 => ArchivedJournalEntry::SocketBindV1(
                rkyv::archived_root::<JournalEntrySocketBindV1>(data),
            ),
            JournalEntryRecordType::SocketConnectedV1 => {
                ArchivedJournalEntry::SocketConnectedV1(rkyv::archived_root::<
                    JournalEntrySocketConnectedV1,
                >(data))
            }
            JournalEntryRecordType::SocketAcceptedV1 => {
                ArchivedJournalEntry::SocketAcceptedV1(rkyv::archived_root::<
                    JournalEntrySocketAcceptedV1,
                >(data))
            }
            JournalEntryRecordType::SocketJoinIpv4MulticastV1 => {
                ArchivedJournalEntry::SocketJoinIpv4MulticastV1(rkyv::archived_root::<
                    JournalEntrySocketJoinIpv4MulticastV1,
                >(data))
            }
            JournalEntryRecordType::SocketJoinIpv6MulticastV1 => {
                ArchivedJournalEntry::SocketJoinIpv6MulticastV1(rkyv::archived_root::<
                    JournalEntrySocketJoinIpv6MulticastV1,
                >(data))
            }
            JournalEntryRecordType::SocketLeaveIpv4MulticastV1 => {
                ArchivedJournalEntry::SocketLeaveIpv4MulticastV1(rkyv::archived_root::<
                    JournalEntrySocketLeaveIpv4MulticastV1,
                >(data))
            }
            JournalEntryRecordType::SocketLeaveIpv6MulticastV1 => {
                ArchivedJournalEntry::SocketLeaveIpv6MulticastV1(rkyv::archived_root::<
                    JournalEntrySocketLeaveIpv6MulticastV1,
                >(data))
            }
            JournalEntryRecordType::SocketSendFileV1 => {
                ArchivedJournalEntry::SocketSendFileV1(rkyv::archived_root::<
                    JournalEntrySocketSendFileV1,
                >(data))
            }
            JournalEntryRecordType::SocketSendToV1 => {
                ArchivedJournalEntry::SocketSendToV1(rkyv::archived_root::<
                    JournalEntrySocketSendToV1,
                >(data))
            }
            JournalEntryRecordType::SocketSendV1 => ArchivedJournalEntry::SocketSendV1(
                rkyv::archived_root::<JournalEntrySocketSendV1>(data),
            ),
            JournalEntryRecordType::SocketSetOptFlagV1 => {
                ArchivedJournalEntry::SocketSetOptFlagV1(rkyv::archived_root::<
                    JournalEntrySocketSetOptFlagV1,
                >(data))
            }
            JournalEntryRecordType::SocketSetOptSizeV1 => {
                ArchivedJournalEntry::SocketSetOptSizeV1(rkyv::archived_root::<
                    JournalEntrySocketSetOptSizeV1,
                >(data))
            }
            JournalEntryRecordType::SocketSetOptTimeV1 => {
                ArchivedJournalEntry::SocketSetOptTimeV1(rkyv::archived_root::<
                    JournalEntrySocketSetOptTimeV1,
                >(data))
            }
            JournalEntryRecordType::SocketShutdownV1 => {
                ArchivedJournalEntry::SocketShutdownV1(rkyv::archived_root::<
                    JournalEntrySocketShutdownV1,
                >(data))
            }
            JournalEntryRecordType::SnapshotV1 => ArchivedJournalEntry::SnapshotV1(
                rkyv::archived_root::<JournalEntrySnapshotV1>(data),
            ),
        }
        .try_into()
    }
}

impl<'a> JournalEntry<'a> {
    pub fn archive_record_type(&self) -> JournalEntryRecordType {
        match self {
            Self::InitModuleV1 { .. } => JournalEntryRecordType::InitModuleV1,
            Self::UpdateMemoryRegionV1 { .. } => JournalEntryRecordType::UpdateMemoryRegionV1,
            Self::ProcessExitV1 { .. } => JournalEntryRecordType::ProcessExitV1,
            Self::SetThreadV1 { .. } => JournalEntryRecordType::SetThreadV1,
            Self::CloseThreadV1 { .. } => JournalEntryRecordType::CloseThreadV1,
            Self::FileDescriptorSeekV1 { .. } => JournalEntryRecordType::FileDescriptorSeekV1,
            Self::FileDescriptorWriteV1 { .. } => JournalEntryRecordType::FileDescriptorWriteV1,
            Self::SetClockTimeV1 { .. } => JournalEntryRecordType::SetClockTimeV1,
            Self::CloseFileDescriptorV1 { .. } => JournalEntryRecordType::CloseFileDescriptorV1,
            Self::OpenFileDescriptorV1 { .. } => JournalEntryRecordType::OpenFileDescriptorV1,
            Self::RenumberFileDescriptorV1 { .. } => {
                JournalEntryRecordType::RenumberFileDescriptorV1
            }
            Self::DuplicateFileDescriptorV1 { .. } => {
                JournalEntryRecordType::DuplicateFileDescriptorV1
            }
            Self::CreateDirectoryV1 { .. } => JournalEntryRecordType::CreateDirectoryV1,
            Self::RemoveDirectoryV1 { .. } => JournalEntryRecordType::RemoveDirectoryV1,
            Self::PathSetTimesV1 { .. } => JournalEntryRecordType::PathSetTimesV1,
            Self::FileDescriptorSetTimesV1 { .. } => {
                JournalEntryRecordType::FileDescriptorSetTimesV1
            }
            Self::FileDescriptorSetFlagsV1 { .. } => {
                JournalEntryRecordType::FileDescriptorSetFlagsV1
            }
            Self::FileDescriptorSetRightsV1 { .. } => {
                JournalEntryRecordType::FileDescriptorSetRightsV1
            }
            Self::FileDescriptorSetSizeV1 { .. } => JournalEntryRecordType::FileDescriptorSetSizeV1,
            Self::FileDescriptorAdviseV1 { .. } => JournalEntryRecordType::FileDescriptorAdviseV1,
            Self::FileDescriptorAllocateV1 { .. } => {
                JournalEntryRecordType::FileDescriptorAllocateV1
            }
            Self::CreateHardLinkV1 { .. } => JournalEntryRecordType::CreateHardLinkV1,
            Self::CreateSymbolicLinkV1 { .. } => JournalEntryRecordType::CreateSymbolicLinkV1,
            Self::UnlinkFileV1 { .. } => JournalEntryRecordType::UnlinkFileV1,
            Self::PathRenameV1 { .. } => JournalEntryRecordType::PathRenameV1,
            Self::ChangeDirectoryV1 { .. } => JournalEntryRecordType::ChangeDirectoryV1,
            Self::EpollCreateV1 { .. } => JournalEntryRecordType::EpollCreateV1,
            Self::EpollCtlV1 { .. } => JournalEntryRecordType::EpollCtlV1,
            Self::TtySetV1 { .. } => JournalEntryRecordType::TtySetV1,
            Self::CreatePipeV1 { .. } => JournalEntryRecordType::CreatePipeV1,
            Self::CreateEventV1 { .. } => JournalEntryRecordType::CreateEventV1,
            Self::PortAddAddrV1 { .. } => JournalEntryRecordType::PortAddAddrV1,
            Self::PortDelAddrV1 { .. } => JournalEntryRecordType::PortDelAddrV1,
            Self::PortAddrClearV1 => JournalEntryRecordType::PortAddrClearV1,
            Self::PortBridgeV1 { .. } => JournalEntryRecordType::PortBridgeV1,
            Self::PortUnbridgeV1 => JournalEntryRecordType::PortUnbridgeV1,
            Self::PortDhcpAcquireV1 => JournalEntryRecordType::PortDhcpAcquireV1,
            Self::PortGatewaySetV1 { .. } => JournalEntryRecordType::PortGatewaySetV1,
            Self::PortRouteAddV1 { .. } => JournalEntryRecordType::PortRouteAddV1,
            Self::PortRouteClearV1 => JournalEntryRecordType::PortRouteClearV1,
            Self::PortRouteDelV1 { .. } => JournalEntryRecordType::PortRouteDelV1,
            Self::SocketOpenV1 { .. } => JournalEntryRecordType::SocketOpenV1,
            Self::SocketListenV1 { .. } => JournalEntryRecordType::SocketListenV1,
            Self::SocketBindV1 { .. } => JournalEntryRecordType::SocketBindV1,
            Self::SocketConnectedV1 { .. } => JournalEntryRecordType::SocketConnectedV1,
            Self::SocketAcceptedV1 { .. } => JournalEntryRecordType::SocketAcceptedV1,
            Self::SocketJoinIpv4MulticastV1 { .. } => {
                JournalEntryRecordType::SocketJoinIpv4MulticastV1
            }
            Self::SocketJoinIpv6MulticastV1 { .. } => {
                JournalEntryRecordType::SocketJoinIpv6MulticastV1
            }
            Self::SocketLeaveIpv4MulticastV1 { .. } => {
                JournalEntryRecordType::SocketLeaveIpv4MulticastV1
            }
            Self::SocketLeaveIpv6MulticastV1 { .. } => {
                JournalEntryRecordType::SocketLeaveIpv6MulticastV1
            }
            Self::SocketSendFileV1 { .. } => JournalEntryRecordType::SocketSendFileV1,
            Self::SocketSendToV1 { .. } => JournalEntryRecordType::SocketSendToV1,
            Self::SocketSendV1 { .. } => JournalEntryRecordType::SocketSendV1,
            Self::SocketSetOptFlagV1 { .. } => JournalEntryRecordType::SocketSetOptFlagV1,
            Self::SocketSetOptSizeV1 { .. } => JournalEntryRecordType::SocketSetOptSizeV1,
            Self::SocketSetOptTimeV1 { .. } => JournalEntryRecordType::SocketSetOptTimeV1,
            Self::SocketShutdownV1 { .. } => JournalEntryRecordType::SocketShutdownV1,
            Self::SnapshotV1 { .. } => JournalEntryRecordType::SnapshotV1,
        }
    }

    pub fn serialize_archive<T: Serializer + ScratchSpace>(
        self,
        serializer: &mut T,
    ) -> anyhow::Result<()>
    where
        T::Error: std::fmt::Display,
    {
        let padding = |size: usize| {
            let padding = size % 16;
            let padding = match padding {
                0 => 0,
                a => 16 - a,
            };
            vec![0u8; padding]
        };
        match self {
            JournalEntry::InitModuleV1 { wasm_hash } => {
                serializer.serialize_value(&JournalEntryInitModuleV1 { wasm_hash })
            }
            JournalEntry::UpdateMemoryRegionV1 { region, data } => {
                serializer.serialize_value(&JournalEntryUpdateMemoryRegionV1 {
                    start: region.start,
                    end: region.end,
                    _padding: padding(data.len()),
                    compressed_data: compress_prepend_size(data.as_ref()),
                })
            }
            JournalEntry::ProcessExitV1 { exit_code } => {
                serializer.serialize_value(&JournalEntryProcessExitV1 {
                    exit_code: exit_code.map(|e| e.into()),
                    _padding: 0,
                })
            }
            JournalEntry::SetThreadV1 {
                id,
                call_stack,
                memory_stack,
                store_data,
                is_64bit,
            } => serializer.serialize_value(&JournalEntrySetThreadV1 {
                id: id.into(),
                _padding: padding(call_stack.len() + memory_stack.len() + store_data.len()),
                call_stack: call_stack.into_owned(),
                memory_stack: memory_stack.into_owned(),
                store_data: store_data.into_owned(),
                is_64bit,
            }),
            JournalEntry::CloseThreadV1 { id, exit_code } => {
                serializer.serialize_value(&JournalEntryCloseThreadV1 {
                    id: id.into(),
                    exit_code: exit_code.map(|e| e.into()),
                })
            }
            JournalEntry::FileDescriptorSeekV1 { fd, offset, whence } => serializer
                .serialize_value(&JournalEntryFileDescriptorSeekV1 {
                    fd,
                    offset,
                    whence: whence.into(),
                }),
            JournalEntry::FileDescriptorWriteV1 {
                fd,
                offset,
                data,
                is_64bit,
            } => serializer.serialize_value(&JournalEntryFileDescriptorWriteV1 {
                fd,
                offset,
                _padding: padding(data.len()),
                data: data.into_owned(),
                is_64bit,
            }),
            JournalEntry::SetClockTimeV1 { clock_id, time } => {
                serializer.serialize_value(&JournalEntrySetClockTimeV1 {
                    clock_id: clock_id.into(),
                    time,
                })
            }
            JournalEntry::CloseFileDescriptorV1 { fd } => {
                serializer.serialize_value(&JournalEntryCloseFileDescriptorV1 { fd, _padding: 0 })
            }
            JournalEntry::OpenFileDescriptorV1 {
                fd,
                dirfd,
                dirflags,
                path,
                o_flags,
                fs_rights_base,
                fs_rights_inheriting,
                fs_flags,
            } => serializer.serialize_value(&JournalEntryOpenFileDescriptorV1 {
                fd,
                dirfd,
                dirflags,
                _padding: padding(path.as_bytes().len()),
                path: path.into_owned(),
                o_flags: o_flags.bits(),
                fs_rights_base: fs_rights_base.bits(),
                fs_rights_inheriting: fs_rights_inheriting.bits(),
                fs_flags: fs_flags.bits(),
            }),
            JournalEntry::RenumberFileDescriptorV1 { old_fd, new_fd } => {
                serializer.serialize_value(&JournalEntryRenumberFileDescriptorV1 { old_fd, new_fd })
            }
            JournalEntry::DuplicateFileDescriptorV1 {
                original_fd,
                copied_fd,
            } => serializer.serialize_value(&JournalEntryDuplicateFileDescriptorV1 {
                original_fd,
                copied_fd,
            }),
            JournalEntry::CreateDirectoryV1 { fd, path } => {
                serializer.serialize_value(&JournalEntryCreateDirectoryV1 {
                    fd,
                    _padding: padding(path.as_bytes().len()),
                    path: path.into_owned(),
                })
            }
            JournalEntry::RemoveDirectoryV1 { fd, path } => {
                serializer.serialize_value(&JournalEntryRemoveDirectoryV1 {
                    fd,
                    _padding: padding(path.as_bytes().len()),
                    path: path.into_owned(),
                })
            }
            JournalEntry::PathSetTimesV1 {
                fd,
                flags,
                path,
                st_atim,
                st_mtim,
                fst_flags,
            } => serializer.serialize_value(&JournalEntryPathSetTimesV1 {
                fd,
                flags,
                _padding: padding(path.as_bytes().len()),
                path: path.into_owned(),
                st_atim,
                st_mtim,
                fst_flags: fst_flags.bits(),
            }),
            JournalEntry::FileDescriptorSetTimesV1 {
                fd,
                st_atim,
                st_mtim,
                fst_flags,
            } => serializer.serialize_value(&JournalEntryFileDescriptorSetTimesV1 {
                fd,
                st_atim,
                st_mtim,
                fst_flags: fst_flags.bits(),
            }),
            JournalEntry::FileDescriptorSetFlagsV1 { fd, flags } => {
                serializer.serialize_value(&JournalEntryFileDescriptorSetFlagsV1 {
                    fd,
                    flags: flags.bits(),
                })
            }
            JournalEntry::FileDescriptorSetRightsV1 {
                fd,
                fs_rights_base,
                fs_rights_inheriting,
            } => serializer.serialize_value(&JournalEntryFileDescriptorSetRightsV1 {
                fd,
                fs_rights_base: fs_rights_base.bits(),
                fs_rights_inheriting: fs_rights_inheriting.bits(),
            }),
            JournalEntry::FileDescriptorSetSizeV1 { fd, st_size } => {
                serializer.serialize_value(&JournalEntryFileDescriptorSetSizeV1 { fd, st_size })
            }
            JournalEntry::FileDescriptorAdviseV1 {
                fd,
                offset,
                len,
                advice,
            } => serializer.serialize_value(&JournalEntryFileDescriptorAdviseV1 {
                fd,
                offset,
                len,
                advice: advice.into(),
            }),
            JournalEntry::FileDescriptorAllocateV1 { fd, offset, len } => serializer
                .serialize_value(&JournalEntryFileDescriptorAllocateV1 { fd, offset, len }),
            JournalEntry::CreateHardLinkV1 {
                old_fd,
                old_path,
                old_flags,
                new_fd,
                new_path,
            } => serializer.serialize_value(&JournalEntryCreateHardLinkV1 {
                old_fd,
                _padding: padding(old_path.as_bytes().len() + new_path.as_bytes().len()),
                old_path: old_path.into_owned(),
                old_flags,
                new_fd,
                new_path: new_path.into_owned(),
            }),
            JournalEntry::CreateSymbolicLinkV1 {
                old_path,
                fd,
                new_path,
            } => serializer.serialize_value(&JournalEntryCreateSymbolicLinkV1 {
                _padding: padding(old_path.as_bytes().len() + new_path.as_bytes().len()),
                old_path: old_path.into_owned(),
                fd,
                new_path: new_path.into_owned(),
            }),
            JournalEntry::UnlinkFileV1 { fd, path } => {
                serializer.serialize_value(&JournalEntryUnlinkFileV1 {
                    fd,
                    _padding: padding(path.as_bytes().len()),
                    path: path.into_owned(),
                })
            }
            JournalEntry::PathRenameV1 {
                old_fd,
                old_path,
                new_fd,
                new_path,
            } => serializer.serialize_value(&JournalEntryPathRenameV1 {
                old_fd,
                _padding: padding(old_path.as_bytes().len() + new_path.as_bytes().len()),
                old_path: old_path.into_owned(),
                new_fd,
                new_path: new_path.into_owned(),
            }),
            JournalEntry::ChangeDirectoryV1 { path } => {
                serializer.serialize_value(&JournalEntryChangeDirectoryV1 {
                    path: path.into_owned(),
                })
            }
            JournalEntry::EpollCreateV1 { fd } => {
                serializer.serialize_value(&JournalEntryEpollCreateV1 { fd, _padding: 0 })
            }
            JournalEntry::EpollCtlV1 {
                epfd,
                op,
                fd,
                event,
            } => serializer.serialize_value(&JournalEntryEpollCtlV1 {
                epfd,
                op: op.into(),
                fd,
                event: event.map(|e| e.into()),
            }),
            JournalEntry::TtySetV1 { tty, line_feeds } => {
                serializer.serialize_value(&JournalEntryTtySetV1 {
                    cols: tty.cols,
                    rows: tty.rows,
                    width: tty.width,
                    height: tty.height,
                    stdin_tty: tty.stdin_tty,
                    stdout_tty: tty.stdout_tty,
                    stderr_tty: tty.stderr_tty,
                    echo: tty.echo,
                    line_buffered: tty.line_buffered,
                    line_feeds,
                })
            }
            JournalEntry::CreatePipeV1 { fd1, fd2 } => {
                serializer.serialize_value(&JournalEntryCreatePipeV1 { fd1, fd2 })
            }
            JournalEntry::CreateEventV1 {
                initial_val,
                flags,
                fd,
            } => serializer.serialize_value(&JournalEntryCreateEventV1 {
                initial_val,
                flags,
                fd,
            }),
            JournalEntry::PortAddAddrV1 { cidr } => {
                serializer.serialize_value(&JournalEntryPortAddAddrV1 { cidr })
            }
            JournalEntry::PortDelAddrV1 { addr } => {
                serializer.serialize_value(&JournalEntryPortDelAddrV1 { addr })
            }
            JournalEntry::PortAddrClearV1 => return Ok(()),
            JournalEntry::PortBridgeV1 {
                network,
                token,
                security,
            } => serializer.serialize_value(&JournalEntryPortBridgeV1 {
                _padding: padding(network.as_bytes().len() + token.as_bytes().len()),
                network: network.into_owned(),
                token: token.into_owned(),
                security: security.into(),
            }),
            JournalEntry::PortUnbridgeV1 => return Ok(()),
            JournalEntry::PortDhcpAcquireV1 => return Ok(()),
            JournalEntry::PortGatewaySetV1 { ip } => {
                serializer.serialize_value(&JournalEntryPortGatewaySetV1 { ip })
            }
            JournalEntry::PortRouteAddV1 {
                cidr,
                via_router,
                preferred_until,
                expires_at,
            } => serializer.serialize_value(&JournalEntryPortRouteAddV1 {
                cidr,
                via_router,
                preferred_until,
                expires_at,
            }),
            JournalEntry::PortRouteClearV1 => return Ok(()),
            JournalEntry::PortRouteDelV1 { ip } => {
                serializer.serialize_value(&JournalEntryPortRouteDelV1 { ip })
            }
            JournalEntry::SocketOpenV1 { af, ty, pt, fd } => {
                serializer.serialize_value(&JournalEntrySocketOpenV1 {
                    af: af.into(),
                    ty: ty.into(),
                    pt: pt.into(),
                    fd,
                })
            }
            JournalEntry::SocketListenV1 { fd, backlog } => {
                serializer.serialize_value(&JournalEntrySocketListenV1 { fd, backlog })
            }
            JournalEntry::SocketBindV1 { fd, addr } => {
                serializer.serialize_value(&JournalEntrySocketBindV1 { fd, addr })
            }
            JournalEntry::SocketConnectedV1 { fd, addr } => {
                serializer.serialize_value(&JournalEntrySocketConnectedV1 { fd, addr })
            }
            JournalEntry::SocketAcceptedV1 {
                listen_fd,
                fd,
                peer_addr,
                fd_flags,
                non_blocking: nonblocking,
            } => serializer.serialize_value(&JournalEntrySocketAcceptedV1 {
                listen_fd,
                fd,
                peer_addr,
                fd_flags: fd_flags.bits(),
                nonblocking,
            }),
            JournalEntry::SocketJoinIpv4MulticastV1 {
                fd,
                multiaddr,
                iface,
            } => serializer.serialize_value(&JournalEntrySocketJoinIpv4MulticastV1 {
                fd,
                multiaddr,
                iface,
            }),
            JournalEntry::SocketJoinIpv6MulticastV1 {
                fd,
                multi_addr: multiaddr,
                iface,
            } => serializer.serialize_value(&JournalEntrySocketJoinIpv6MulticastV1 {
                fd,
                multiaddr,
                iface,
            }),
            JournalEntry::SocketLeaveIpv4MulticastV1 {
                fd,
                multi_addr: multiaddr,
                iface,
            } => serializer.serialize_value(&JournalEntrySocketLeaveIpv4MulticastV1 {
                fd,
                multiaddr,
                iface,
            }),
            JournalEntry::SocketLeaveIpv6MulticastV1 {
                fd,
                multi_addr: multiaddr,
                iface,
            } => serializer.serialize_value(&JournalEntrySocketLeaveIpv6MulticastV1 {
                fd,
                multiaddr,
                iface,
            }),
            JournalEntry::SocketSendFileV1 {
                socket_fd,
                file_fd,
                offset,
                count,
            } => serializer.serialize_value(&JournalEntrySocketSendFileV1 {
                socket_fd,
                file_fd,
                offset,
                count,
            }),
            JournalEntry::SocketSendToV1 {
                fd,
                data,
                flags,
                addr,
                is_64bit,
            } => serializer.serialize_value(&JournalEntrySocketSendToV1 {
                fd,
                _padding: padding(data.len()),
                data: data.into_owned(),
                flags,
                addr,
                is_64bit,
            }),
            JournalEntry::SocketSendV1 {
                fd,
                data,
                flags,
                is_64bit,
            } => serializer.serialize_value(&JournalEntrySocketSendV1 {
                fd,
                _padding: padding(data.len()),
                data: data.into_owned(),
                flags,
                is_64bit,
            }),
            JournalEntry::SocketSetOptFlagV1 { fd, opt, flag } => {
                serializer.serialize_value(&JournalEntrySocketSetOptFlagV1 {
                    fd,
                    opt: opt.into(),
                    flag,
                })
            }
            JournalEntry::SocketSetOptSizeV1 { fd, opt, size } => {
                serializer.serialize_value(&JournalEntrySocketSetOptSizeV1 {
                    fd,
                    opt: opt.into(),
                    size,
                })
            }
            JournalEntry::SocketSetOptTimeV1 { fd, ty, time } => {
                serializer.serialize_value(&JournalEntrySocketSetOptTimeV1 {
                    fd,
                    ty: ty.into(),
                    time,
                })
            }
            JournalEntry::SocketShutdownV1 { fd, how } => {
                serializer.serialize_value(&JournalEntrySocketShutdownV1 {
                    fd,
                    how: how.into(),
                })
            }
            JournalEntry::SnapshotV1 { when, trigger } => {
                serializer.serialize_value(&JournalEntrySnapshotV1 {
                    since_epoch: when
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap_or(Duration::ZERO),
                    trigger: trigger.into(),
                })
            }
        }
        .map_err(|err| anyhow::format_err!("failed to serialize journal record - {}", err))?;
        Ok(())
    }
}

/// The journal log entries are serializable which
/// allows them to be written directly to a file
///
/// Note: This structure is versioned which allows for
/// changes to the journal entry types without having to
/// worry about backward and forward compatibility
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub(crate) struct JournalEntryHeader {
    pub record_type: u16,
    pub record_size: u64,
}

pub enum ArchivedJournalEntry<'a> {
    InitModuleV1(&'a ArchivedJournalEntryInitModuleV1),
    ProcessExitV1(&'a ArchivedJournalEntryProcessExitV1),
    SetThreadV1(&'a ArchivedJournalEntrySetThreadV1),
    CloseThreadV1(&'a ArchivedJournalEntryCloseThreadV1),
    FileDescriptorSeekV1(&'a ArchivedJournalEntryFileDescriptorSeekV1),
    FileDescriptorWriteV1(&'a ArchivedJournalEntryFileDescriptorWriteV1),
    UpdateMemoryRegionV1(&'a ArchivedJournalEntryUpdateMemoryRegionV1),
    SetClockTimeV1(&'a ArchivedJournalEntrySetClockTimeV1),
    OpenFileDescriptorV1(&'a ArchivedJournalEntryOpenFileDescriptorV1),
    CloseFileDescriptorV1(&'a ArchivedJournalEntryCloseFileDescriptorV1),
    RenumberFileDescriptorV1(&'a ArchivedJournalEntryRenumberFileDescriptorV1),
    DuplicateFileDescriptorV1(&'a ArchivedJournalEntryDuplicateFileDescriptorV1),
    CreateDirectoryV1(&'a ArchivedJournalEntryCreateDirectoryV1),
    RemoveDirectoryV1(&'a ArchivedJournalEntryRemoveDirectoryV1),
    PathSetTimesV1(&'a ArchivedJournalEntryPathSetTimesV1),
    FileDescriptorSetTimesV1(&'a ArchivedJournalEntryFileDescriptorSetTimesV1),
    FileDescriptorSetSizeV1(&'a ArchivedJournalEntryFileDescriptorSetSizeV1),
    FileDescriptorSetFlagsV1(&'a ArchivedJournalEntryFileDescriptorSetFlagsV1),
    FileDescriptorSetRightsV1(&'a ArchivedJournalEntryFileDescriptorSetRightsV1),
    FileDescriptorAdviseV1(&'a ArchivedJournalEntryFileDescriptorAdviseV1),
    FileDescriptorAllocateV1(&'a ArchivedJournalEntryFileDescriptorAllocateV1),
    CreateHardLinkV1(&'a ArchivedJournalEntryCreateHardLinkV1),
    CreateSymbolicLinkV1(&'a ArchivedJournalEntryCreateSymbolicLinkV1),
    UnlinkFileV1(&'a ArchivedJournalEntryUnlinkFileV1),
    PathRenameV1(&'a ArchivedJournalEntryPathRenameV1),
    ChangeDirectoryV1(&'a ArchivedJournalEntryChangeDirectoryV1),
    EpollCreateV1(&'a ArchivedJournalEntryEpollCreateV1),
    EpollCtlV1(&'a ArchivedJournalEntryEpollCtlV1),
    TtySetV1(&'a ArchivedJournalEntryTtySetV1),
    CreatePipeV1(&'a ArchivedJournalEntryCreatePipeV1),
    CreateEventV1(&'a ArchivedJournalEntryCreateEventV1),
    PortAddAddrV1(&'a ArchivedJournalEntryPortAddAddrV1),
    PortDelAddrV1(&'a ArchivedJournalEntryPortDelAddrV1),
    PortAddrClearV1,
    PortBridgeV1(&'a ArchivedJournalEntryPortBridgeV1),
    PortUnbridgeV1,
    PortDhcpAcquireV1,
    PortGatewaySetV1(&'a ArchivedJournalEntryPortGatewaySetV1),
    PortRouteAddV1(&'a ArchivedJournalEntryPortRouteAddV1),
    PortRouteClearV1,
    PortRouteDelV1(&'a ArchivedJournalEntryPortRouteDelV1),
    SocketOpenV1(&'a ArchivedJournalEntrySocketOpenV1),
    SocketListenV1(&'a ArchivedJournalEntrySocketListenV1),
    SocketBindV1(&'a ArchivedJournalEntrySocketBindV1),
    SocketConnectedV1(&'a ArchivedJournalEntrySocketConnectedV1),
    SocketAcceptedV1(&'a ArchivedJournalEntrySocketAcceptedV1),
    SocketJoinIpv4MulticastV1(&'a ArchivedJournalEntrySocketJoinIpv4MulticastV1),
    SocketJoinIpv6MulticastV1(&'a ArchivedJournalEntrySocketJoinIpv6MulticastV1),
    SocketLeaveIpv4MulticastV1(&'a ArchivedJournalEntrySocketLeaveIpv4MulticastV1),
    SocketLeaveIpv6MulticastV1(&'a ArchivedJournalEntrySocketLeaveIpv6MulticastV1),
    SocketSendFileV1(&'a ArchivedJournalEntrySocketSendFileV1),
    SocketSendToV1(&'a ArchivedJournalEntrySocketSendToV1),
    SocketSendV1(&'a ArchivedJournalEntrySocketSendV1),
    SocketSetOptFlagV1(&'a ArchivedJournalEntrySocketSetOptFlagV1),
    SocketSetOptSizeV1(&'a ArchivedJournalEntrySocketSetOptSizeV1),
    SocketSetOptTimeV1(&'a ArchivedJournalEntrySocketSetOptTimeV1),
    SocketShutdownV1(&'a ArchivedJournalEntrySocketShutdownV1),
    SnapshotV1(&'a ArchivedJournalEntrySnapshotV1),
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntryInitModuleV1 {
    pub wasm_hash: [u8; 8],
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntryProcessExitV1 {
    pub exit_code: Option<JournalExitCodeV1>,
    pub _padding: u32,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntrySetThreadV1 {
    pub id: u32,
    pub call_stack: Vec<u8>,
    pub memory_stack: Vec<u8>,
    pub store_data: Vec<u8>,
    pub _padding: Vec<u8>,
    pub is_64bit: bool,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntryCloseThreadV1 {
    pub id: u32,
    pub exit_code: Option<JournalExitCodeV1>,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntryFileDescriptorSeekV1 {
    pub fd: u32,
    pub offset: i64,
    pub whence: JournalWhenceV1,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntryFileDescriptorWriteV1 {
    pub fd: u32,
    pub offset: u64,
    pub data: Vec<u8>,
    pub _padding: Vec<u8>,
    pub is_64bit: bool,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntryUpdateMemoryRegionV1 {
    pub start: u64,
    pub end: u64,
    pub compressed_data: Vec<u8>,
    pub _padding: Vec<u8>,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntrySetClockTimeV1 {
    pub clock_id: JournalSnapshot0ClockidV1,
    pub time: u64,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntryOpenFileDescriptorV1 {
    pub fd: u32,
    pub dirfd: u32,
    pub dirflags: u32,
    pub path: String,
    pub _padding: Vec<u8>,
    pub o_flags: u16,
    pub fs_rights_base: u64,
    pub fs_rights_inheriting: u64,
    pub fs_flags: u16,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntryCloseFileDescriptorV1 {
    pub fd: u32,
    pub _padding: u32,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntryRenumberFileDescriptorV1 {
    pub old_fd: u32,
    pub new_fd: u32,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntryDuplicateFileDescriptorV1 {
    pub original_fd: u32,
    pub copied_fd: u32,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntryCreateDirectoryV1 {
    pub fd: u32,
    pub path: String,
    pub _padding: Vec<u8>,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntryRemoveDirectoryV1 {
    pub fd: u32,
    pub path: String,
    pub _padding: Vec<u8>,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntryPathSetTimesV1 {
    pub fd: u32,
    pub flags: u32,
    pub path: String,
    pub _padding: Vec<u8>,
    pub st_atim: u64,
    pub st_mtim: u64,
    pub fst_flags: u16,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntryFileDescriptorSetTimesV1 {
    pub fd: u32,
    pub st_atim: u64,
    pub st_mtim: u64,
    pub fst_flags: u16,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntryFileDescriptorSetSizeV1 {
    pub fd: u32,
    pub st_size: u64,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntryFileDescriptorSetFlagsV1 {
    pub fd: u32,
    pub flags: u16,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntryFileDescriptorSetRightsV1 {
    pub fd: u32,
    pub fs_rights_base: u64,
    pub fs_rights_inheriting: u64,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntryFileDescriptorAdviseV1 {
    pub fd: u32,
    pub offset: u64,
    pub len: u64,
    pub advice: JournalAdviceV1,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntryFileDescriptorAllocateV1 {
    pub fd: u32,
    pub offset: u64,
    pub len: u64,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntryCreateHardLinkV1 {
    pub old_fd: u32,
    pub old_path: String,
    pub old_flags: u32,
    pub new_fd: u32,
    pub new_path: String,
    pub _padding: Vec<u8>,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntryCreateSymbolicLinkV1 {
    pub old_path: String,
    pub fd: u32,
    pub new_path: String,
    pub _padding: Vec<u8>,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntryUnlinkFileV1 {
    pub fd: u32,
    pub path: String,
    pub _padding: Vec<u8>,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntryPathRenameV1 {
    pub old_fd: u32,
    pub old_path: String,
    pub new_fd: u32,
    pub new_path: String,
    pub _padding: Vec<u8>,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntryChangeDirectoryV1 {
    pub path: String,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntryEpollCreateV1 {
    pub fd: u32,
    pub _padding: u32,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntryEpollCtlV1 {
    pub epfd: u32,
    pub op: JournalEpollCtlV1,
    pub fd: u32,
    pub event: Option<JournalEpollEventCtlV1>,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntryTtySetV1 {
    pub cols: u32,
    pub rows: u32,
    pub width: u32,
    pub height: u32,
    pub stdin_tty: bool,
    pub stdout_tty: bool,
    pub stderr_tty: bool,
    pub echo: bool,
    pub line_buffered: bool,
    pub line_feeds: bool,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntryCreatePipeV1 {
    pub fd1: u32,
    pub fd2: u32,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntryCreateEventV1 {
    pub initial_val: u64,
    pub flags: u16,
    pub fd: u32,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntryPortAddAddrV1 {
    pub cidr: IpCidr,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntryPortDelAddrV1 {
    pub addr: IpAddr,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntryPortBridgeV1 {
    pub network: String,
    pub token: String,
    pub _padding: Vec<u8>,
    pub security: JournalStreamSecurityV1,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntryPortGatewaySetV1 {
    pub ip: IpAddr,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntryPortRouteAddV1 {
    pub cidr: IpCidr,
    pub via_router: IpAddr,
    pub preferred_until: Option<Duration>,
    pub expires_at: Option<Duration>,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntryPortRouteDelV1 {
    pub ip: IpAddr,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntrySocketOpenV1 {
    pub af: JournalAddressfamilyV1,
    pub ty: JournalSocktypeV1,
    pub pt: u16,
    pub fd: u32,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntrySocketListenV1 {
    pub fd: u32,
    pub backlog: u32,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntrySocketBindV1 {
    pub fd: u32,
    pub addr: SocketAddr,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntrySocketConnectedV1 {
    pub fd: u32,
    pub addr: SocketAddr,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntrySocketAcceptedV1 {
    pub listen_fd: u32,
    pub fd: u32,
    pub peer_addr: SocketAddr,
    pub fd_flags: u16,
    pub nonblocking: bool,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntrySocketJoinIpv4MulticastV1 {
    pub fd: u32,
    pub multiaddr: Ipv4Addr,
    pub iface: Ipv4Addr,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntrySocketJoinIpv6MulticastV1 {
    pub fd: u32,
    pub multiaddr: Ipv6Addr,
    pub iface: u32,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntrySocketLeaveIpv4MulticastV1 {
    pub fd: u32,
    pub multiaddr: Ipv4Addr,
    pub iface: Ipv4Addr,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntrySocketLeaveIpv6MulticastV1 {
    pub fd: u32,
    pub multiaddr: Ipv6Addr,
    pub iface: u32,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntrySocketSendFileV1 {
    pub socket_fd: u32,
    pub file_fd: u32,
    pub offset: u64,
    pub count: u64,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntrySocketSendToV1 {
    pub fd: u32,
    pub data: Vec<u8>,
    pub _padding: Vec<u8>,
    pub flags: u16,
    pub addr: SocketAddr,
    pub is_64bit: bool,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntrySocketSendV1 {
    pub fd: u32,
    pub data: Vec<u8>,
    pub _padding: Vec<u8>,
    pub flags: u16,
    pub is_64bit: bool,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntrySocketSetOptFlagV1 {
    pub fd: u32,
    pub opt: JournalSockoptionV1,
    pub flag: bool,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntrySocketSetOptSizeV1 {
    pub fd: u32,
    pub opt: JournalSockoptionV1,
    pub size: u64,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntrySocketSetOptTimeV1 {
    pub fd: u32,
    pub ty: JournalTimeTypeV1,
    pub time: Option<Duration>,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntrySocketShutdownV1 {
    pub fd: u32,
    pub how: JournalSocketShutdownV1,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEntrySnapshotV1 {
    pub since_epoch: Duration,
    pub trigger: JournalSnapshotTriggerV1,
}

#[repr(C)]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub enum JournalSnapshot0ClockidV1 {
    Realtime,
    Monotonic,
    ProcessCputimeId,
    ThreadCputimeId,
    Unknown = 255,
}

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

#[repr(C)]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub enum JournalWhenceV1 {
    Set,
    Cur,
    End,
    Unknown = 255,
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

#[repr(C)]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub enum JournalAdviceV1 {
    Normal,
    Sequential,
    Random,
    Willneed,
    Dontneed,
    Noreuse,
    Unknown = 255,
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

#[repr(C)]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub enum JournalExitCodeV1 {
    Errno(u16),
    Other(i32),
}

impl From<wasi::ExitCode> for JournalExitCodeV1 {
    fn from(val: wasi::ExitCode) -> Self {
        match val {
            wasi::ExitCode::Errno(errno) => JournalExitCodeV1::Errno(errno as u16),
            wasi::ExitCode::Other(id) => JournalExitCodeV1::Other(id),
        }
    }
}

impl From<JournalExitCodeV1> for wasi::ExitCode {
    fn from(val: JournalExitCodeV1) -> Self {
        match val {
            JournalExitCodeV1::Errno(errno) => {
                wasi::ExitCode::Errno(errno.try_into().unwrap_or(wasi::Errno::Unknown))
            }
            JournalExitCodeV1::Other(id) => wasi::ExitCode::Other(id),
        }
    }
}

impl From<&'_ ArchivedJournalExitCodeV1> for wasi::ExitCode {
    fn from(val: &'_ ArchivedJournalExitCodeV1) -> Self {
        match val {
            ArchivedJournalExitCodeV1::Errno(errno) => {
                wasi::ExitCode::Errno((*errno).try_into().unwrap_or(wasi::Errno::Unknown))
            }
            ArchivedJournalExitCodeV1::Other(id) => wasi::ExitCode::Other(*id),
        }
    }
}

#[repr(C)]
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    RkyvSerialize,
    RkyvDeserialize,
    Archive,
)]
#[archive_attr(derive(CheckBytes))]
pub enum JournalSnapshotTriggerV1 {
    Idle,
    Listen,
    Environ,
    Stdin,
    Timer,
    Sigint,
    Sigalrm,
    Sigtstp,
    Sigstop,
    NonDeterministicCall,
}

impl From<SnapshotTrigger> for JournalSnapshotTriggerV1 {
    fn from(val: SnapshotTrigger) -> Self {
        match val {
            SnapshotTrigger::Idle => JournalSnapshotTriggerV1::Idle,
            SnapshotTrigger::FirstListen => JournalSnapshotTriggerV1::Listen,
            SnapshotTrigger::FirstEnviron => JournalSnapshotTriggerV1::Environ,
            SnapshotTrigger::FirstStdin => JournalSnapshotTriggerV1::Stdin,
            SnapshotTrigger::PeriodicInterval => JournalSnapshotTriggerV1::Timer,
            SnapshotTrigger::Sigint => JournalSnapshotTriggerV1::Sigint,
            SnapshotTrigger::Sigalrm => JournalSnapshotTriggerV1::Sigalrm,
            SnapshotTrigger::Sigtstp => JournalSnapshotTriggerV1::Sigtstp,
            SnapshotTrigger::Sigstop => JournalSnapshotTriggerV1::Sigstop,
            SnapshotTrigger::NonDeterministicCall => JournalSnapshotTriggerV1::NonDeterministicCall,
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
        }
    }
}

#[repr(C)]
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    RkyvSerialize,
    RkyvDeserialize,
    Archive,
)]
#[archive_attr(derive(CheckBytes))]
pub enum JournalEpollCtlV1 {
    Add,
    Mod,
    Del,
    Unknown,
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

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalEpollEventCtlV1 {
    pub events: u32,
    pub ptr: u64,
    pub fd: u32,
    pub data1: u32,
    pub data2: u64,
}

impl From<EpollEventCtl> for JournalEpollEventCtlV1 {
    fn from(val: EpollEventCtl) -> Self {
        JournalEpollEventCtlV1 {
            events: val.events.bits(),
            ptr: val.ptr,
            fd: val.fd,
            data1: val.data1,
            data2: val.data2,
        }
    }
}

impl From<JournalEpollEventCtlV1> for EpollEventCtl {
    fn from(val: JournalEpollEventCtlV1) -> Self {
        Self {
            events: EpollType::from_bits_truncate(val.events),
            ptr: val.ptr,
            fd: val.fd,
            data1: val.data1,
            data2: val.data2,
        }
    }
}

impl From<&'_ ArchivedJournalEpollEventCtlV1> for EpollEventCtl {
    fn from(val: &'_ ArchivedJournalEpollEventCtlV1) -> Self {
        Self {
            events: EpollType::from_bits_truncate(val.events),
            ptr: val.ptr,
            fd: val.fd,
            data1: val.data1,
            data2: val.data2,
        }
    }
}

#[repr(C)]
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    RkyvSerialize,
    RkyvDeserialize,
    Archive,
)]
#[archive_attr(derive(CheckBytes))]
pub enum JournalStreamSecurityV1 {
    Unencrypted,
    AnyEncryption,
    ClassicEncryption,
    DoubleEncryption,
    Unknown,
}

impl From<StreamSecurity> for JournalStreamSecurityV1 {
    fn from(val: StreamSecurity) -> Self {
        match val {
            StreamSecurity::Unencrypted => JournalStreamSecurityV1::Unencrypted,
            StreamSecurity::AnyEncyption => JournalStreamSecurityV1::AnyEncryption,
            StreamSecurity::ClassicEncryption => JournalStreamSecurityV1::ClassicEncryption,
            StreamSecurity::DoubleEncryption => JournalStreamSecurityV1::DoubleEncryption,
        }
    }
}

impl From<JournalStreamSecurityV1> for StreamSecurity {
    fn from(val: JournalStreamSecurityV1) -> Self {
        match val {
            JournalStreamSecurityV1::Unencrypted => StreamSecurity::Unencrypted,
            JournalStreamSecurityV1::AnyEncryption => StreamSecurity::AnyEncyption,
            JournalStreamSecurityV1::ClassicEncryption => StreamSecurity::ClassicEncryption,
            JournalStreamSecurityV1::DoubleEncryption => StreamSecurity::DoubleEncryption,
            JournalStreamSecurityV1::Unknown => StreamSecurity::AnyEncyption,
        }
    }
}

impl From<&'_ ArchivedJournalStreamSecurityV1> for StreamSecurity {
    fn from(val: &'_ ArchivedJournalStreamSecurityV1) -> Self {
        match val {
            ArchivedJournalStreamSecurityV1::Unencrypted => StreamSecurity::Unencrypted,
            ArchivedJournalStreamSecurityV1::AnyEncryption => StreamSecurity::AnyEncyption,
            ArchivedJournalStreamSecurityV1::ClassicEncryption => StreamSecurity::ClassicEncryption,
            ArchivedJournalStreamSecurityV1::DoubleEncryption => StreamSecurity::DoubleEncryption,
            ArchivedJournalStreamSecurityV1::Unknown => StreamSecurity::AnyEncyption,
        }
    }
}

#[repr(C)]
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    RkyvSerialize,
    RkyvDeserialize,
    Archive,
)]
#[archive_attr(derive(CheckBytes))]
pub enum JournalAddressfamilyV1 {
    Unspec,
    Inet4,
    Inet6,
    Unix,
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

#[repr(C)]
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    RkyvSerialize,
    RkyvDeserialize,
    Archive,
)]
#[archive_attr(derive(CheckBytes))]
pub enum JournalSocktypeV1 {
    Unknown,
    Stream,
    Dgram,
    Raw,
    Seqpacket,
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

#[repr(C)]
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    RkyvSerialize,
    RkyvDeserialize,
    Archive,
)]
#[archive_attr(derive(CheckBytes))]
pub enum JournalSockoptionV1 {
    Noop,
    ReusePort,
    ReuseAddr,
    NoDelay,
    DontRoute,
    OnlyV6,
    Broadcast,
    MulticastLoopV4,
    MulticastLoopV6,
    Promiscuous,
    Listening,
    LastError,
    KeepAlive,
    Linger,
    OobInline,
    RecvBufSize,
    SendBufSize,
    RecvLowat,
    SendLowat,
    RecvTimeout,
    SendTimeout,
    ConnectTimeout,
    AcceptTimeout,
    Ttl,
    MulticastTtlV4,
    Type,
    Proto,
}

impl From<Sockoption> for JournalSockoptionV1 {
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

#[repr(C)]
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    RkyvSerialize,
    RkyvDeserialize,
    Archive,
)]
#[archive_attr(derive(CheckBytes))]
pub enum JournalTimeTypeV1 {
    ReadTimeout,
    WriteTimeout,
    AcceptTimeout,
    ConnectTimeout,
    BindTimeout,
    Linger,
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

#[repr(C)]
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    RkyvSerialize,
    RkyvDeserialize,
    Archive,
)]
#[archive_attr(derive(CheckBytes))]
pub enum JournalSocketShutdownV1 {
    Read,
    Write,
    Both,
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

impl<'a> TryFrom<ArchivedJournalEntry<'a>> for JournalEntry<'a> {
    type Error = anyhow::Error;

    fn try_from(value: ArchivedJournalEntry<'a>) -> anyhow::Result<Self> {
        Ok(match value {
            ArchivedJournalEntry::InitModuleV1(ArchivedJournalEntryInitModuleV1 { wasm_hash }) => {
                Self::InitModuleV1 {
                    wasm_hash: *wasm_hash,
                }
            }
            ArchivedJournalEntry::UpdateMemoryRegionV1(
                ArchivedJournalEntryUpdateMemoryRegionV1 {
                    start,
                    end,
                    compressed_data,
                    _padding: _,
                },
            ) => Self::UpdateMemoryRegionV1 {
                region: (*start)..(*end),
                data: Cow::Owned(decompress_size_prepended(compressed_data.as_ref())?),
            },
            ArchivedJournalEntry::ProcessExitV1(ArchivedJournalEntryProcessExitV1 {
                exit_code,
                _padding: _,
            }) => Self::ProcessExitV1 {
                exit_code: exit_code.as_ref().map(|code| code.into()),
            },
            ArchivedJournalEntry::SetThreadV1(ArchivedJournalEntrySetThreadV1 {
                id,
                call_stack,
                memory_stack,
                store_data,
                _padding: _,
                is_64bit,
            }) => Self::SetThreadV1 {
                id: (*id).into(),
                call_stack: call_stack.as_ref().into(),
                memory_stack: memory_stack.as_ref().into(),
                store_data: store_data.as_ref().into(),
                is_64bit: *is_64bit,
            },
            ArchivedJournalEntry::CloseThreadV1(ArchivedJournalEntryCloseThreadV1 {
                id,
                exit_code,
            }) => Self::CloseThreadV1 {
                id: (*id).into(),
                exit_code: exit_code.as_ref().map(|code| code.into()),
            },
            ArchivedJournalEntry::FileDescriptorWriteV1(
                ArchivedJournalEntryFileDescriptorWriteV1 {
                    data,
                    fd,
                    offset,
                    is_64bit,
                    _padding: _,
                },
            ) => Self::FileDescriptorWriteV1 {
                data: data.as_ref().into(),
                fd: *fd,
                offset: *offset,
                is_64bit: *is_64bit,
            },
            ArchivedJournalEntry::FileDescriptorSeekV1(
                ArchivedJournalEntryFileDescriptorSeekV1 {
                    fd,
                    offset,
                    ref whence,
                },
            ) => Self::FileDescriptorSeekV1 {
                fd: *fd,
                offset: *offset,
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
                    _padding: _,
                },
            ) => Self::OpenFileDescriptorV1 {
                fd: *fd,
                dirfd: *dirfd,
                dirflags: *dirflags,
                path: path.as_ref().into(),
                o_flags: wasi::Oflags::from_bits_truncate(*o_flags),
                fs_rights_base: wasi::Rights::from_bits_truncate(*fs_rights_base),
                fs_rights_inheriting: wasi::Rights::from_bits_truncate(*fs_rights_inheriting),
                fs_flags: wasi::Fdflags::from_bits_truncate(*fs_flags),
            },
            ArchivedJournalEntry::CloseFileDescriptorV1(
                ArchivedJournalEntryCloseFileDescriptorV1 { fd, _padding: _ },
            ) => Self::CloseFileDescriptorV1 { fd: *fd },
            ArchivedJournalEntry::RemoveDirectoryV1(ArchivedJournalEntryRemoveDirectoryV1 {
                fd,
                path,
                _padding: _,
            }) => Self::RemoveDirectoryV1 {
                fd: *fd,
                path: path.as_ref().into(),
            },
            ArchivedJournalEntry::UnlinkFileV1(ArchivedJournalEntryUnlinkFileV1 {
                fd,
                path,
                _padding: _,
            }) => Self::UnlinkFileV1 {
                fd: *fd,
                path: path.as_ref().into(),
            },
            ArchivedJournalEntry::PathRenameV1(ArchivedJournalEntryPathRenameV1 {
                old_fd,
                old_path,
                new_fd,
                new_path,
                _padding: _,
            }) => Self::PathRenameV1 {
                old_fd: *old_fd,
                old_path: old_path.as_ref().into(),
                new_fd: *new_fd,
                new_path: new_path.as_ref().into(),
            },
            ArchivedJournalEntry::SnapshotV1(ArchivedJournalEntrySnapshotV1 {
                since_epoch,
                ref trigger,
            }) => Self::SnapshotV1 {
                when: SystemTime::UNIX_EPOCH
                    .checked_add((*since_epoch).try_into().unwrap())
                    .unwrap_or(SystemTime::UNIX_EPOCH),
                trigger: trigger.into(),
            },
            ArchivedJournalEntry::SetClockTimeV1(ArchivedJournalEntrySetClockTimeV1 {
                ref clock_id,
                time,
            }) => Self::SetClockTimeV1 {
                clock_id: clock_id.into(),
                time: *time,
            },
            ArchivedJournalEntry::RenumberFileDescriptorV1(
                ArchivedJournalEntryRenumberFileDescriptorV1 { old_fd, new_fd },
            ) => Self::RenumberFileDescriptorV1 {
                old_fd: *old_fd,
                new_fd: *new_fd,
            },
            ArchivedJournalEntry::DuplicateFileDescriptorV1(
                ArchivedJournalEntryDuplicateFileDescriptorV1 {
                    original_fd: old_fd,
                    copied_fd: new_fd,
                },
            ) => Self::DuplicateFileDescriptorV1 {
                original_fd: *old_fd,
                copied_fd: *new_fd,
            },
            ArchivedJournalEntry::CreateDirectoryV1(ArchivedJournalEntryCreateDirectoryV1 {
                fd,
                path,
                _padding: _,
            }) => Self::CreateDirectoryV1 {
                fd: *fd,
                path: path.as_ref().into(),
            },
            ArchivedJournalEntry::PathSetTimesV1(ArchivedJournalEntryPathSetTimesV1 {
                fd,
                path,
                flags,
                st_atim,
                st_mtim,
                fst_flags,
                _padding: _,
            }) => Self::PathSetTimesV1 {
                fd: *fd,
                path: path.as_ref().into(),
                flags: *flags,
                st_atim: *st_atim,
                st_mtim: *st_mtim,
                fst_flags: wasi::Fstflags::from_bits_truncate(*fst_flags),
            },
            ArchivedJournalEntry::FileDescriptorSetTimesV1(
                ArchivedJournalEntryFileDescriptorSetTimesV1 {
                    fd,
                    st_atim,
                    st_mtim,
                    fst_flags,
                },
            ) => Self::FileDescriptorSetTimesV1 {
                fd: *fd,
                st_atim: *st_atim,
                st_mtim: *st_mtim,
                fst_flags: wasi::Fstflags::from_bits_truncate(*fst_flags),
            },
            ArchivedJournalEntry::FileDescriptorSetSizeV1(
                ArchivedJournalEntryFileDescriptorSetSizeV1 { fd, st_size },
            ) => Self::FileDescriptorSetSizeV1 {
                fd: *fd,
                st_size: *st_size,
            },
            ArchivedJournalEntry::FileDescriptorSetFlagsV1(
                ArchivedJournalEntryFileDescriptorSetFlagsV1 { fd, flags },
            ) => Self::FileDescriptorSetFlagsV1 {
                fd: *fd,
                flags: Fdflags::from_bits_truncate(*flags),
            },
            ArchivedJournalEntry::FileDescriptorSetRightsV1(
                ArchivedJournalEntryFileDescriptorSetRightsV1 {
                    fd,
                    fs_rights_base,
                    fs_rights_inheriting,
                },
            ) => Self::FileDescriptorSetRightsV1 {
                fd: *fd,
                fs_rights_base: Rights::from_bits_truncate(*fs_rights_base),
                fs_rights_inheriting: Rights::from_bits_truncate(*fs_rights_inheriting),
            },
            ArchivedJournalEntry::FileDescriptorAdviseV1(
                ArchivedJournalEntryFileDescriptorAdviseV1 {
                    fd,
                    offset,
                    len,
                    ref advice,
                },
            ) => Self::FileDescriptorAdviseV1 {
                fd: *fd,
                offset: *offset,
                len: *len,
                advice: advice.into(),
            },
            ArchivedJournalEntry::FileDescriptorAllocateV1(
                ArchivedJournalEntryFileDescriptorAllocateV1 { fd, offset, len },
            ) => Self::FileDescriptorAllocateV1 {
                fd: *fd,
                offset: *offset,
                len: *len,
            },
            ArchivedJournalEntry::CreateHardLinkV1(ArchivedJournalEntryCreateHardLinkV1 {
                old_fd,
                old_path,
                old_flags,
                new_fd,
                new_path,
                _padding: _,
            }) => Self::CreateHardLinkV1 {
                old_fd: *old_fd,
                old_path: old_path.as_ref().into(),
                old_flags: *old_flags,
                new_fd: *new_fd,
                new_path: new_path.as_ref().into(),
            },
            ArchivedJournalEntry::CreateSymbolicLinkV1(
                ArchivedJournalEntryCreateSymbolicLinkV1 {
                    old_path,
                    fd,
                    new_path,
                    _padding: _,
                },
            ) => Self::CreateSymbolicLinkV1 {
                old_path: old_path.as_ref().into(),
                fd: *fd,
                new_path: new_path.as_ref().into(),
            },
            ArchivedJournalEntry::ChangeDirectoryV1(ArchivedJournalEntryChangeDirectoryV1 {
                path,
            }) => Self::ChangeDirectoryV1 {
                path: path.as_ref().into(),
            },
            ArchivedJournalEntry::EpollCreateV1(ArchivedJournalEntryEpollCreateV1 {
                fd,
                _padding: _,
            }) => Self::EpollCreateV1 { fd: *fd },
            ArchivedJournalEntry::EpollCtlV1(ArchivedJournalEntryEpollCtlV1 {
                epfd,
                ref op,
                fd,
                ref event,
            }) => Self::EpollCtlV1 {
                epfd: *epfd,
                op: op.into(),
                fd: *fd,
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
                    cols: *cols,
                    rows: *rows,
                    width: *width,
                    height: *height,
                    stdin_tty: *stdin_tty,
                    stdout_tty: *stdout_tty,
                    stderr_tty: *stderr_tty,
                    echo: *echo,
                    line_buffered: *line_buffered,
                },
                line_feeds: *line_feeds,
            },
            ArchivedJournalEntry::CreatePipeV1(ArchivedJournalEntryCreatePipeV1 { fd1, fd2 }) => {
                Self::CreatePipeV1 {
                    fd1: *fd1,
                    fd2: *fd2,
                }
            }
            ArchivedJournalEntry::PortAddAddrV1(ArchivedJournalEntryPortAddAddrV1 { cidr }) => {
                Self::PortAddAddrV1 {
                    cidr: IpCidr {
                        ip: cidr.ip.as_ipaddr(),
                        prefix: cidr.prefix,
                    },
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
                _padding: _,
            }) => Self::PortBridgeV1 {
                network: network.as_ref().into(),
                token: token.as_ref().into(),
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
                cidr: IpCidr {
                    ip: cidr.ip.as_ipaddr(),
                    prefix: cidr.prefix,
                },
                via_router: via_router.as_ipaddr(),
                preferred_until: preferred_until
                    .as_ref()
                    .map(|time| (*time).try_into().unwrap()),
                expires_at: expires_at.as_ref().map(|time| (*time).try_into().unwrap()),
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
                pt: (*pt).try_into().unwrap_or(wasi::SockProto::Max),
                fd: *fd,
            },
            ArchivedJournalEntry::SocketListenV1(ArchivedJournalEntrySocketListenV1 {
                fd,
                backlog,
            }) => Self::SocketListenV1 {
                fd: *fd,
                backlog: *backlog,
            },
            ArchivedJournalEntry::SocketBindV1(ArchivedJournalEntrySocketBindV1 { fd, addr }) => {
                Self::SocketBindV1 {
                    fd: *fd,
                    addr: addr.as_socket_addr(),
                }
            }
            ArchivedJournalEntry::SocketConnectedV1(ArchivedJournalEntrySocketConnectedV1 {
                fd,
                addr,
            }) => Self::SocketConnectedV1 {
                fd: *fd,
                addr: addr.as_socket_addr(),
            },
            ArchivedJournalEntry::SocketAcceptedV1(ArchivedJournalEntrySocketAcceptedV1 {
                listen_fd,
                fd,
                peer_addr,
                fd_flags,
                nonblocking,
            }) => Self::SocketAcceptedV1 {
                listen_fd: *listen_fd,
                fd: *fd,
                peer_addr: peer_addr.as_socket_addr(),
                fd_flags: Fdflags::from_bits_truncate(*fd_flags),
                non_blocking: *nonblocking,
            },
            ArchivedJournalEntry::SocketJoinIpv4MulticastV1(
                ArchivedJournalEntrySocketJoinIpv4MulticastV1 {
                    fd,
                    multiaddr,
                    iface,
                },
            ) => Self::SocketJoinIpv4MulticastV1 {
                fd: *fd,
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
                fd: *fd,
                multi_addr: multiaddr.as_ipv6(),
                iface: *iface,
            },
            ArchivedJournalEntry::SocketLeaveIpv4MulticastV1(
                ArchivedJournalEntrySocketLeaveIpv4MulticastV1 {
                    fd,
                    multiaddr,
                    iface,
                },
            ) => Self::SocketLeaveIpv4MulticastV1 {
                fd: *fd,
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
                fd: *fd,
                multi_addr: multiaddr.as_ipv6(),
                iface: *iface,
            },
            ArchivedJournalEntry::SocketSendFileV1(ArchivedJournalEntrySocketSendFileV1 {
                socket_fd,
                file_fd,
                offset,
                count,
            }) => Self::SocketSendFileV1 {
                socket_fd: *socket_fd,
                file_fd: *file_fd,
                offset: *offset,
                count: *count,
            },
            ArchivedJournalEntry::SocketSendToV1(ArchivedJournalEntrySocketSendToV1 {
                fd,
                data,
                flags,
                addr,
                is_64bit,
                _padding: _,
            }) => Self::SocketSendToV1 {
                fd: *fd,
                data: data.as_ref().into(),
                flags: *flags,
                addr: addr.as_socket_addr(),
                is_64bit: *is_64bit,
            },
            ArchivedJournalEntry::SocketSendV1(ArchivedJournalEntrySocketSendV1 {
                fd,
                data,
                flags,
                is_64bit,
                _padding: _,
            }) => Self::SocketSendV1 {
                fd: *fd,
                data: data.as_ref().into(),
                flags: *flags,
                is_64bit: *is_64bit,
            },
            ArchivedJournalEntry::SocketSetOptFlagV1(ArchivedJournalEntrySocketSetOptFlagV1 {
                fd,
                ref opt,
                flag,
            }) => Self::SocketSetOptFlagV1 {
                fd: *fd,
                opt: opt.into(),
                flag: *flag,
            },
            ArchivedJournalEntry::SocketSetOptSizeV1(ArchivedJournalEntrySocketSetOptSizeV1 {
                fd,
                ref opt,
                size,
            }) => Self::SocketSetOptSizeV1 {
                fd: *fd,
                opt: opt.into(),
                size: *size,
            },
            ArchivedJournalEntry::SocketSetOptTimeV1(ArchivedJournalEntrySocketSetOptTimeV1 {
                fd,
                ref ty,
                time,
            }) => Self::SocketSetOptTimeV1 {
                fd: *fd,
                ty: ty.into(),
                time: time.as_ref().map(|time| (*time).try_into().unwrap()),
            },
            ArchivedJournalEntry::SocketShutdownV1(ArchivedJournalEntrySocketShutdownV1 {
                fd,
                ref how,
            }) => Self::SocketShutdownV1 {
                fd: *fd,
                how: how.into(),
            },
            ArchivedJournalEntry::CreateEventV1(ArchivedJournalEntryCreateEventV1 {
                initial_val,
                flags,
                fd,
            }) => Self::CreateEventV1 {
                initial_val: *initial_val,
                flags: *flags,
                fd: *fd,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use rkyv::ser::serializers::{
        AllocScratch, CompositeSerializer, SharedSerializeMap, WriteSerializer,
    };

    use super::*;

    pub fn run_test<'a>(record: JournalEntry<'a>) {
        tracing::info!("record: {:?}", record);

        // Determine the record type
        let record_type = record.archive_record_type();
        tracing::info!("record_type: {:?}", record_type);

        // Serialize it
        let mut buffer = Vec::new();
        let mut serializer = CompositeSerializer::new(
            WriteSerializer::new(&mut buffer),
            AllocScratch::default(),
            SharedSerializeMap::default(),
        );

        record.clone().serialize_archive(&mut serializer).unwrap();
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
            wasm_hash: [13u8; 8],
        });
    }

    #[tracing_test::traced_test]
    #[test]
    pub fn test_record_process_exit() {
        run_test(JournalEntry::ProcessExitV1 {
            exit_code: Some(wasi::ExitCode::Errno(wasi::Errno::Fault)),
        });
    }

    #[tracing_test::traced_test]
    #[test]
    pub fn test_record_set_thread() {
        run_test(JournalEntry::SetThreadV1 {
            id: 1234u32.into(),
            call_stack: vec![1, 2, 3].into(),
            memory_stack: vec![4, 5, 6, 7].into(),
            store_data: vec![10, 11].into(),
            is_64bit: true,
        });
    }

    #[tracing_test::traced_test]
    #[test]
    pub fn test_record_close_thread() {
        run_test(JournalEntry::CloseThreadV1 {
            id: 987u32.into(),
            exit_code: Some(wasi::ExitCode::Errno(wasi::Errno::Fault)),
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
            data: [74u8; 40960].to_vec().into(),
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
            fd1: 3452345,
            fd2: 2345163,
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
            cidr: IpCidr {
                ip: Ipv4Addr::LOCALHOST.into(),
                prefix: 24,
            },
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
            token: format!("blh blah").into(),
            security: StreamSecurity::ClassicEncryption,
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
            cidr: IpCidr {
                ip: Ipv4Addr::LOCALHOST.into(),
                prefix: 24,
            },
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
            addr: SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), 1234),
        });
    }

    #[tracing_test::traced_test]
    #[test]
    pub fn test_record_socket_accepted() {
        run_test(JournalEntry::SocketAcceptedV1 {
            listen_fd: 21234,
            fd: 1,
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
            multiaddr: Ipv4Addr::new(123, 123, 123, 123).into(),
            iface: Ipv4Addr::new(128, 0, 0, 1).into(),
        });
    }

    #[tracing_test::traced_test]
    #[test]
    pub fn test_record_socket_join_ipv6_multicast() {
        run_test(JournalEntry::SocketJoinIpv6MulticastV1 {
            fd: 12,
            multi_addr: Ipv6Addr::new(123, 123, 123, 123, 1234, 12663, 31, 1324).into(),
            iface: 23541,
        });
    }

    #[tracing_test::traced_test]
    #[test]
    pub fn test_record_socket_leave_ipv4_multicast() {
        run_test(JournalEntry::SocketLeaveIpv4MulticastV1 {
            fd: 12,
            multi_addr: Ipv4Addr::new(123, 123, 123, 123).into(),
            iface: Ipv4Addr::new(128, 0, 0, 1).into(),
        });
    }

    #[tracing_test::traced_test]
    #[test]
    pub fn test_record_socket_leave_ipv6_multicast() {
        run_test(JournalEntry::SocketLeaveIpv6MulticastV1 {
            fd: 12,
            multi_addr: Ipv6Addr::new(123, 123, 123, 123, 1234, 12663, 31, 1324).into(),
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
}
