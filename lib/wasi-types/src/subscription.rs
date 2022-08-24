use wasmer_wasi_types_generated::wasi::{Eventtype, SubscriptionClock, SubscriptionFsReadwrite};

/// Safe Rust wrapper around `__wasi_subscription_t::type_` and `__wasi_subscription_t::u`
#[derive(Debug, Clone)]
pub enum EventType {
    Clock(SubscriptionClock),
    Read(SubscriptionFsReadwrite),
    Write(SubscriptionFsReadwrite),
}

impl EventType {
    pub fn raw_tag(&self) -> Eventtype {
        match self {
            EventType::Clock(_) => Eventtype::Clock,
            EventType::Read(_) => Eventtype::FdRead,
            EventType::Write(_) => Eventtype::FdWrite,
        }
    }
}

/* TODO: re-enable and adjust if still required
impl TryFrom<WasiSubscription> for __wasi_subscription_t {
    type Error = Errno;

    fn try_from(ws: WasiSubscription) -> Result<Self, Self::Error> {
        #[allow(unreachable_patterns)]
        let (type_, u) = match ws.event_type {
            EventType::Clock(c) => (Eventtype::Clock, __wasi_subscription_u { clock: c }),
            EventType::Read(rw) => (
                Eventtype::FdRead,
                __wasi_subscription_u { fd_readwrite: rw },
            ),
            EventType::Write(rw) => (
                Eventtype::FdWrite,
                __wasi_subscription_u { fd_readwrite: rw },
            ),
            _ => return Err(Errno::Inval),
        };

        Ok(Self {
            userdata: ws.user_data,
            type_,
            u,
        })
    }
}

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

unsafe impl ValueType for __wasi_subscription_t {
    fn zero_padding_bytes(&self, bytes: &mut [MaybeUninit<u8>]) {
        macro_rules! field {
            ($($f:tt)*) => {
                &self.$($f)* as *const _ as usize - self as *const _ as usize
            };
        }
        macro_rules! field_end {
            ($($f:tt)*) => {
                field!($($f)*) + mem::size_of_val(&self.$($f)*)
            };
        }
        macro_rules! zero {
            ($start:expr, $end:expr) => {
                for i in $start..$end {
                    bytes[i] = MaybeUninit::new(0);
                }
            };
        }
        self.userdata
            .zero_padding_bytes(&mut bytes[field!(userdata)..field_end!(userdata)]);
        zero!(field_end!(userdata), field!(type_));
        self.type_
            .zero_padding_bytes(&mut bytes[field!(type_)..field_end!(type_)]);
        zero!(field_end!(type_), field!(u));
        match self.type_ {
            Eventtype::FdRead | Eventtype::FdWrite => unsafe {
                self.u.fd_readwrite.zero_padding_bytes(
                    &mut bytes[field!(u.fd_readwrite)..field_end!(u.fd_readwrite)],
                );
                zero!(field_end!(u.fd_readwrite), field_end!(u));
            },
            Eventtype::Clock => unsafe {
                self.u
                    .clock
                    .zero_padding_bytes(&mut bytes[field!(u.clock)..field_end!(u.clock)]);
                zero!(field_end!(u.clock), field_end!(u));
            },
        }
        zero!(field_end!(u), mem::size_of_val(self));
    }
}

pub enum SubscriptionEnum {
    Clock(__wasi_subscription_clock_t),
    FdReadWrite(__wasi_subscription_fs_readwrite_t),
}

impl __wasi_subscription_t {
    pub fn tagged(&self) -> Option<SubscriptionEnum> {
        match self.type_ {
            Eventtype::Clock => Some(SubscriptionEnum::Clock(unsafe { self.u.clock })),
            Eventtype::FdRead | Eventtype::FdWrite => Some(SubscriptionEnum::FdReadWrite(unsafe {
                self.u.fd_readwrite
            })),
        }
    }
}

*/
