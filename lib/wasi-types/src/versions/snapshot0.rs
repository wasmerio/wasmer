use std::fmt;
use wasmer_derive::ValueType;
use wasmer_wasi_types_generated::wasi::{
    Device, Filesize, Filetype, Inode, Snapshot0Linkcount, Timestamp,
};

pub type __wasi_whence_t = u8;
pub const __WASI_WHENCE_CUR: u8 = 0;
pub const __WASI_WHENCE_END: u8 = 1;
pub const __WASI_WHENCE_SET: u8 = 2;

#[derive(Copy, Clone, PartialEq, Eq, ValueType)]
#[repr(C)]
pub struct __wasi_filestat_t {
    pub st_dev: Device,
    pub st_ino: Inode,
    pub st_filetype: Filetype,
    pub st_nlink: Snapshot0Linkcount,
    pub st_size: Filesize,
    pub st_atim: Timestamp,
    pub st_mtim: Timestamp,
    pub st_ctim: Timestamp,
}

impl fmt::Debug for __wasi_filestat_t {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let convert_ts_into_time_string = |ts| {
            let tspec = ::time::OffsetDateTime::from_unix_timestamp_nanos(ts);
            format!("{} ({})", tspec.format("%a, %d %b %Y %T %z"), ts)
        };
        f.debug_struct("__wasi_filestat_t")
            .field("st_dev", &self.st_dev)
            .field("st_ino", &self.st_ino)
            .field(
                "st_filetype",
                &format!("{} ({})", self.st_filetype.name(), self.st_filetype as u8,),
            )
            .field("st_nlink", &self.st_nlink)
            .field("st_size", &self.st_size)
            .field(
                "st_atim",
                &convert_ts_into_time_string(self.st_atim as i128),
            )
            .field(
                "st_mtim",
                &convert_ts_into_time_string(self.st_mtim as i128),
            )
            .field(
                "st_ctim",
                &convert_ts_into_time_string(self.st_ctim as i128),
            )
            .finish()
    }
}

/* TODO: re-enable and adjust if required
impl fmt::Debug for __wasi_subscription_t {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("__wasi_subscription_t")
            .field("userdata", &self.userdata)
            .field("type", &self.type_.to_str())
            .field(
                "u",
                match self.type_ {
                    Eventtype::Clock => unsafe { &self.u.clock },
                    Eventtype::FdRead | Eventtype::FdWrite => unsafe { &self.u.fd_readwrite },
                },
            )
            .finish()
    }
}
*/
