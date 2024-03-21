use lz4_flex::block::compress_prepend_size;
use num_enum::{IntoPrimitive, TryFromPrimitive};
use rkyv::ser::{ScratchSpace, Serializer};
use rkyv::{Archive, CheckBytes, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::{Duration, SystemTime};

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
    ClearEtherealV1 = 60,
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
            JournalEntryRecordType::ClearEtherealV1 => {
                ArchivedJournalEntry::ClearEtherealV1(rkyv::archived_root::<
                    JournalEntryClearEtherealV1,
                >(data))
            }
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
            Self::ClearEtherealV1 { .. } => JournalEntryRecordType::ClearEtherealV1,
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
    ) -> anyhow::Result<usize>
    where
        T::Error: std::fmt::Display,
    {
        let amt = match self {
            JournalEntry::InitModuleV1 { wasm_hash } => {
                serializer.serialize_value(&JournalEntryInitModuleV1 { wasm_hash })
            }
            JournalEntry::ClearEtherealV1 => {
                serializer.serialize_value(&JournalEntryClearEtherealV1 {})
            }
            JournalEntry::UpdateMemoryRegionV1 { region, data } => {
                serializer.serialize_value(&JournalEntryUpdateMemoryRegionV1 {
                    start: region.start,
                    end: region.end,
                    compressed_data: compress_prepend_size(data.as_ref()).into(),
                })
            }
            JournalEntry::ProcessExitV1 { exit_code } => {
                serializer.serialize_value(&JournalEntryProcessExitV1 {
                    exit_code: exit_code.map(|e| e.into()),
                })
            }
            JournalEntry::SetThreadV1 {
                id,
                call_stack,
                memory_stack,
                store_data,
                is_64bit,
                start,
                layout,
            } => serializer.serialize_value(&JournalEntrySetThreadV1 {
                id,
                call_stack: call_stack.into(),
                memory_stack: memory_stack.into(),
                store_data: store_data.into(),
                start: start.into(),
                layout: layout.into(),
                is_64bit,
            }),
            JournalEntry::CloseThreadV1 { id, exit_code } => {
                serializer.serialize_value(&JournalEntryCloseThreadV1 {
                    id,
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
                data: data.into(),
                is_64bit,
            }),
            JournalEntry::SetClockTimeV1 { clock_id, time } => {
                serializer.serialize_value(&JournalEntrySetClockTimeV1 {
                    clock_id: clock_id.into(),
                    time,
                })
            }
            JournalEntry::CloseFileDescriptorV1 { fd } => {
                serializer.serialize_value(&JournalEntryCloseFileDescriptorV1 { fd })
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
                path: path.into(),
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
                    path: path.into(),
                })
            }
            JournalEntry::RemoveDirectoryV1 { fd, path } => {
                serializer.serialize_value(&JournalEntryRemoveDirectoryV1 {
                    fd,
                    path: path.into(),
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
                path: path.into(),
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
                old_path: old_path.into(),
                old_flags,
                new_fd,
                new_path: new_path.into(),
            }),
            JournalEntry::CreateSymbolicLinkV1 {
                old_path,
                fd,
                new_path,
            } => serializer.serialize_value(&JournalEntryCreateSymbolicLinkV1 {
                old_path: old_path.into(),
                fd,
                new_path: new_path.into(),
            }),
            JournalEntry::UnlinkFileV1 { fd, path } => {
                serializer.serialize_value(&JournalEntryUnlinkFileV1 {
                    fd,
                    path: path.into(),
                })
            }
            JournalEntry::PathRenameV1 {
                old_fd,
                old_path,
                new_fd,
                new_path,
            } => serializer.serialize_value(&JournalEntryPathRenameV1 {
                old_fd,
                old_path: old_path.into(),
                new_fd,
                new_path: new_path.into(),
            }),
            JournalEntry::ChangeDirectoryV1 { path } => {
                serializer.serialize_value(&JournalEntryChangeDirectoryV1 { path: path.into() })
            }
            JournalEntry::EpollCreateV1 { fd } => {
                serializer.serialize_value(&JournalEntryEpollCreateV1 { fd })
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
                serializer.serialize_value(&JournalEntryPortAddAddrV1 { cidr: cidr.into() })
            }
            JournalEntry::PortDelAddrV1 { addr } => {
                serializer.serialize_value(&JournalEntryPortDelAddrV1 { addr })
            }
            JournalEntry::PortAddrClearV1 => serializer.serialize_value(&()),
            JournalEntry::PortBridgeV1 {
                network,
                token,
                security,
            } => serializer.serialize_value(&JournalEntryPortBridgeV1 {
                network: network.into(),
                token: token.into(),
                security: security.into(),
            }),
            JournalEntry::PortUnbridgeV1 => serializer.serialize_value(&()),
            JournalEntry::PortDhcpAcquireV1 => serializer.serialize_value(&()),
            JournalEntry::PortGatewaySetV1 { ip } => {
                serializer.serialize_value(&JournalEntryPortGatewaySetV1 { ip })
            }
            JournalEntry::PortRouteAddV1 {
                cidr,
                via_router,
                preferred_until,
                expires_at,
            } => serializer.serialize_value(&JournalEntryPortRouteAddV1 {
                cidr: cidr.into(),
                via_router,
                preferred_until,
                expires_at,
            }),
            JournalEntry::PortRouteClearV1 => serializer.serialize_value(&()),
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
            JournalEntry::SocketConnectedV1 {
                fd,
                local_addr,
                peer_addr,
            } => serializer.serialize_value(&JournalEntrySocketConnectedV1 {
                fd,
                local_addr,
                peer_addr,
            }),
            JournalEntry::SocketAcceptedV1 {
                listen_fd,
                fd,
                local_addr: addr,
                peer_addr,
                fd_flags,
                non_blocking: nonblocking,
            } => serializer.serialize_value(&JournalEntrySocketAcceptedV1 {
                listen_fd,
                fd,
                local_addr: addr,
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
                data: data.into(),
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
                data: data.into(),
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
        Ok(amt)
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
    ClearEtherealV1(&'a ArchivedJournalEntryClearEtherealV1),
    ProcessExitV1(&'a ArchivedJournalEntryProcessExitV1),
    SetThreadV1(&'a ArchivedJournalEntrySetThreadV1<'a>),
    CloseThreadV1(&'a ArchivedJournalEntryCloseThreadV1),
    FileDescriptorSeekV1(&'a ArchivedJournalEntryFileDescriptorSeekV1),
    FileDescriptorWriteV1(&'a ArchivedJournalEntryFileDescriptorWriteV1<'a>),
    UpdateMemoryRegionV1(&'a ArchivedJournalEntryUpdateMemoryRegionV1<'a>),
    SetClockTimeV1(&'a ArchivedJournalEntrySetClockTimeV1),
    OpenFileDescriptorV1(&'a ArchivedJournalEntryOpenFileDescriptorV1<'a>),
    CloseFileDescriptorV1(&'a ArchivedJournalEntryCloseFileDescriptorV1),
    RenumberFileDescriptorV1(&'a ArchivedJournalEntryRenumberFileDescriptorV1),
    DuplicateFileDescriptorV1(&'a ArchivedJournalEntryDuplicateFileDescriptorV1),
    CreateDirectoryV1(&'a ArchivedJournalEntryCreateDirectoryV1<'a>),
    RemoveDirectoryV1(&'a ArchivedJournalEntryRemoveDirectoryV1<'a>),
    PathSetTimesV1(&'a ArchivedJournalEntryPathSetTimesV1<'a>),
    FileDescriptorSetTimesV1(&'a ArchivedJournalEntryFileDescriptorSetTimesV1),
    FileDescriptorSetSizeV1(&'a ArchivedJournalEntryFileDescriptorSetSizeV1),
    FileDescriptorSetFlagsV1(&'a ArchivedJournalEntryFileDescriptorSetFlagsV1),
    FileDescriptorSetRightsV1(&'a ArchivedJournalEntryFileDescriptorSetRightsV1),
    FileDescriptorAdviseV1(&'a ArchivedJournalEntryFileDescriptorAdviseV1),
    FileDescriptorAllocateV1(&'a ArchivedJournalEntryFileDescriptorAllocateV1),
    CreateHardLinkV1(&'a ArchivedJournalEntryCreateHardLinkV1<'a>),
    CreateSymbolicLinkV1(&'a ArchivedJournalEntryCreateSymbolicLinkV1<'a>),
    UnlinkFileV1(&'a ArchivedJournalEntryUnlinkFileV1<'a>),
    PathRenameV1(&'a ArchivedJournalEntryPathRenameV1<'a>),
    ChangeDirectoryV1(&'a ArchivedJournalEntryChangeDirectoryV1<'a>),
    EpollCreateV1(&'a ArchivedJournalEntryEpollCreateV1),
    EpollCtlV1(&'a ArchivedJournalEntryEpollCtlV1),
    TtySetV1(&'a ArchivedJournalEntryTtySetV1),
    CreatePipeV1(&'a ArchivedJournalEntryCreatePipeV1),
    CreateEventV1(&'a ArchivedJournalEntryCreateEventV1),
    PortAddAddrV1(&'a ArchivedJournalEntryPortAddAddrV1),
    PortDelAddrV1(&'a ArchivedJournalEntryPortDelAddrV1),
    PortAddrClearV1,
    PortBridgeV1(&'a ArchivedJournalEntryPortBridgeV1<'a>),
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
    SocketSendToV1(&'a ArchivedJournalEntrySocketSendToV1<'a>),
    SocketSendV1(&'a ArchivedJournalEntrySocketSendV1<'a>),
    SocketSetOptFlagV1(&'a ArchivedJournalEntrySocketSetOptFlagV1),
    SocketSetOptSizeV1(&'a ArchivedJournalEntrySocketSetOptSizeV1),
    SocketSetOptTimeV1(&'a ArchivedJournalEntrySocketSetOptTimeV1),
    SocketShutdownV1(&'a ArchivedJournalEntrySocketShutdownV1),
    SnapshotV1(&'a ArchivedJournalEntrySnapshotV1),
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntryInitModuleV1 {
    pub wasm_hash: [u8; 8],
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntryClearEtherealV1 {}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntryProcessExitV1 {
    pub exit_code: Option<JournalExitCodeV1>,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntrySetThreadV1<'a> {
    pub id: u32,
    pub call_stack: AlignedCowVec<'a, u8>,
    pub memory_stack: AlignedCowVec<'a, u8>,
    pub store_data: AlignedCowVec<'a, u8>,
    pub start: JournalThreadStartTypeV1,
    pub layout: JournalWasiMemoryLayout,
    pub is_64bit: bool,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntryCloseThreadV1 {
    pub id: u32,
    pub exit_code: Option<JournalExitCodeV1>,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntryFileDescriptorSeekV1 {
    pub fd: u32,
    pub whence: JournalWhenceV1,
    pub offset: i64,
}

/// WARNING!!!! Do not change this structure without updating
/// "/lib/cli/src/commands/journal/mount/fs.rs"
///
/// The code over there assumes that the aligned vector is the
/// first item in the serialized entry
#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntryFileDescriptorWriteV1<'a> {
    /// DO NOT MOVE!
    pub data: AlignedCowVec<'a, u8>,
    pub offset: u64,
    pub fd: u32,
    pub is_64bit: bool,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntryUpdateMemoryRegionV1<'a> {
    pub compressed_data: AlignedCowVec<'a, u8>,
    pub start: u64,
    pub end: u64,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntrySetClockTimeV1 {
    pub clock_id: JournalSnapshot0ClockidV1,
    pub time: u64,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntryOpenFileDescriptorV1<'a> {
    pub fd: u32,
    pub dirfd: u32,
    pub dirflags: u32,
    pub fs_flags: u16,
    pub o_flags: u16,
    pub fs_rights_base: u64,
    pub fs_rights_inheriting: u64,
    pub path: AlignedCowStr<'a>,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntryCloseFileDescriptorV1 {
    pub fd: u32,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntryRenumberFileDescriptorV1 {
    pub old_fd: u32,
    pub new_fd: u32,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntryDuplicateFileDescriptorV1 {
    pub original_fd: u32,
    pub copied_fd: u32,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntryCreateDirectoryV1<'a> {
    pub fd: u32,
    pub path: AlignedCowStr<'a>,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntryRemoveDirectoryV1<'a> {
    pub fd: u32,
    pub path: AlignedCowStr<'a>,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntryPathSetTimesV1<'a> {
    pub fd: u32,
    pub flags: u32,
    pub path: AlignedCowStr<'a>,
    pub st_atim: u64,
    pub st_mtim: u64,
    pub fst_flags: u16,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntryFileDescriptorSetTimesV1 {
    pub fd: u32,
    pub fst_flags: u16,
    pub st_atim: u64,
    pub st_mtim: u64,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntryFileDescriptorSetSizeV1 {
    pub fd: u32,
    pub st_size: u64,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntryFileDescriptorSetFlagsV1 {
    pub fd: u32,
    pub flags: u16,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntryFileDescriptorSetRightsV1 {
    pub fd: u32,
    pub fs_rights_base: u64,
    pub fs_rights_inheriting: u64,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntryFileDescriptorAdviseV1 {
    pub fd: u32,
    pub offset: u64,
    pub len: u64,
    pub advice: JournalAdviceV1,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntryFileDescriptorAllocateV1 {
    pub fd: u32,
    pub offset: u64,
    pub len: u64,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntryCreateHardLinkV1<'a> {
    pub old_fd: u32,
    pub old_path: AlignedCowStr<'a>,
    pub old_flags: u32,
    pub new_fd: u32,
    pub new_path: AlignedCowStr<'a>,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntryCreateSymbolicLinkV1<'a> {
    pub fd: u32,
    pub old_path: AlignedCowStr<'a>,
    pub new_path: AlignedCowStr<'a>,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntryUnlinkFileV1<'a> {
    pub fd: u32,
    pub path: AlignedCowStr<'a>,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntryPathRenameV1<'a> {
    pub old_fd: u32,
    pub old_path: AlignedCowStr<'a>,
    pub new_fd: u32,
    pub new_path: AlignedCowStr<'a>,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntryChangeDirectoryV1<'a> {
    pub path: AlignedCowStr<'a>,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntryEpollCreateV1 {
    pub fd: u32,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntryEpollCtlV1 {
    pub epfd: u32,
    pub op: JournalEpollCtlV1,
    pub fd: u32,
    pub event: Option<JournalEpollEventCtlV1>,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
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
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntryCreatePipeV1 {
    pub fd1: u32,
    pub fd2: u32,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntryCreateEventV1 {
    pub initial_val: u64,
    pub flags: u16,
    pub fd: u32,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntryPortAddAddrV1 {
    pub cidr: JournalIpCidrV1,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntryPortDelAddrV1 {
    pub addr: IpAddr,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntryPortBridgeV1<'a> {
    pub network: AlignedCowStr<'a>,
    pub token: AlignedCowStr<'a>,
    pub security: JournalStreamSecurityV1,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntryPortGatewaySetV1 {
    pub ip: IpAddr,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntryPortRouteAddV1 {
    pub cidr: JournalIpCidrV1,
    pub via_router: IpAddr,
    pub preferred_until: Option<Duration>,
    pub expires_at: Option<Duration>,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntryPortRouteDelV1 {
    pub ip: IpAddr,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntrySocketOpenV1 {
    pub af: JournalAddressfamilyV1,
    pub ty: JournalSocktypeV1,
    pub pt: u16,
    pub fd: u32,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntrySocketListenV1 {
    pub fd: u32,
    pub backlog: u32,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntrySocketBindV1 {
    pub fd: u32,
    pub addr: SocketAddr,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntrySocketConnectedV1 {
    pub fd: u32,
    pub local_addr: SocketAddr,
    pub peer_addr: SocketAddr,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntrySocketAcceptedV1 {
    pub listen_fd: u32,
    pub fd: u32,
    pub local_addr: SocketAddr,
    pub peer_addr: SocketAddr,
    pub fd_flags: u16,
    pub nonblocking: bool,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntrySocketJoinIpv4MulticastV1 {
    pub fd: u32,
    pub multiaddr: Ipv4Addr,
    pub iface: Ipv4Addr,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntrySocketJoinIpv6MulticastV1 {
    pub fd: u32,
    pub multiaddr: Ipv6Addr,
    pub iface: u32,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntrySocketLeaveIpv4MulticastV1 {
    pub fd: u32,
    pub multiaddr: Ipv4Addr,
    pub iface: Ipv4Addr,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntrySocketLeaveIpv6MulticastV1 {
    pub fd: u32,
    pub multiaddr: Ipv6Addr,
    pub iface: u32,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntrySocketSendFileV1 {
    pub socket_fd: u32,
    pub file_fd: u32,
    pub offset: u64,
    pub count: u64,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntrySocketSendToV1<'a> {
    pub fd: u32,
    pub data: AlignedCowVec<'a, u8>,
    pub flags: u16,
    pub addr: SocketAddr,
    pub is_64bit: bool,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntrySocketSendV1<'a> {
    pub fd: u32,
    pub data: AlignedCowVec<'a, u8>,
    pub flags: u16,
    pub is_64bit: bool,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntrySocketSetOptFlagV1 {
    pub fd: u32,
    pub opt: JournalSockoptionV1,
    pub flag: bool,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntrySocketSetOptSizeV1 {
    pub fd: u32,
    pub opt: JournalSockoptionV1,
    pub size: u64,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntrySocketSetOptTimeV1 {
    pub fd: u32,
    pub ty: JournalTimeTypeV1,
    pub time: Option<Duration>,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalEntrySocketShutdownV1 {
    pub fd: u32,
    pub how: JournalSocketShutdownV1,
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
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

#[repr(C)]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub enum JournalWhenceV1 {
    Set,
    Cur,
    End,
    Unknown = 255,
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

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub struct JournalIpCidrV1 {
    pub ip: IpAddr,
    pub prefix: u8,
}

#[repr(C)]
#[derive(Debug, Clone, RkyvSerialize, RkyvDeserialize, Archive)]
#[archive_attr(derive(CheckBytes))]
pub enum JournalExitCodeV1 {
    Errno(u16),
    Other(i32),
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

#[repr(C)]
#[repr(align(8))]
#[derive(
    Debug,
    Clone,
    Copy,
    RkyvSerialize,
    RkyvDeserialize,
    Archive,
    PartialOrd,
    Ord,
    PartialEq,
    Eq,
    Hash,
)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub enum JournalThreadStartTypeV1 {
    MainThread,
    ThreadSpawn { start_ptr: u64 },
}

#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, Copy, RkyvSerialize, RkyvDeserialize, Archive, PartialEq, Eq, Hash)]
#[archive_attr(derive(CheckBytes), repr(align(8)))]
pub struct JournalWasiMemoryLayout {
    pub stack_upper: u64,
    pub stack_lower: u64,
    pub guard_size: u64,
    pub stack_size: u64,
}
