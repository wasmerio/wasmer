#![cfg(all(target_arch = "wasm32", target_os = "wasi"))]
#![feature(wasi_ext)]

use std::cell::RefCell;
use std::fs::File;
use std::net::{AddrParseError, Ipv4Addr};
use std::os::wasi::io::FromRawFd;
use std::sync::{Arc, Mutex};

const AF_INET: i32 = 2;
const SOCK_STREAM: i32 = 1;
const O_NONBLOCK: u32 = 2048;
const F_GETFL: i32 = 3;
const F_SETFL: i32 = 4;
const EPOLLIN: u32 = 1u32;
const EPOLLOUT: u32 = 4u32;
const EPOLLONESHOT: u32 = 1u32 << 30;
const EPOLLET: u32 = 1u32 << 31;
const EAGAIN: i32 = 11;
const EWOULDBLOCK: i32 = EAGAIN;
const EPOLL_CTL_ADD: i32 = 1;
const EPOLL_CTL_DEL: i32 = 2;

#[link(wasm_import_module = "net")]
extern "C" {
    fn _socket(family: i32, _type: i32, proto: i32) -> i32;
    fn _bind(fd: i32, sa: *const SockaddrIn, sa_len: usize) -> i32;
    fn _listen(fd: i32, backlog: i32) -> i32;
    fn _accept4(fd: i32, sa: *mut SockaddrIn, sa_len: *mut usize, flags: u32) -> i32;
    fn _sendto(
        fd: i32,
        buf: *const u8,
        buf_len: usize,
        flags: u32,
        addr: *const SockaddrIn,
        addr_len: usize,
    ) -> i32;
    fn _recvfrom(
        fd: i32,
        buf: *mut u8,
        buf_len: usize,
        flags: u32,
        addr: *mut SockaddrIn,
        addr_len: *mut usize,
    ) -> i32;
    fn _eventfd_sem(initial: u32) -> i32;
    fn _epoll_create() -> i32;
    fn _epoll_ctl(epfd: i32, op: i32, fd: i32, event: *const EpollEvent) -> i32;
    fn _epoll_wait(epfd: i32, events: *mut EpollEvent, maxevents: usize, timeout: i32) -> i32;
    fn _fcntl(fd: i32, cmd: i32, arg: u32) -> i32;
}

thread_local! {
    static GLOBAL_EPOLL: RefCell<Option<Arc<Epoll>>> = RefCell::new(None);
    static ASYNC_STATE_POOL: RefCell<Vec<Box<AsyncState>>> = RefCell::new(Vec::new());
}

#[derive(Default)]
struct AsyncState {
    callback: Option<Box<FnOnce()>>,
    _epoll: Option<Arc<Epoll>>,
}

pub struct Epoll {
    fd: i32,
    imm_queue: Mutex<Vec<Box<FnOnce()>>>,
}

impl Epoll {
    pub fn new() -> Epoll {
        let fd = unsafe { _epoll_create() };
        assert!(fd >= 0);
        Epoll {
            fd: fd,
            imm_queue: Mutex::new(Vec::new()),
        }
    }

    pub fn schedule<F: FnOnce() + 'static>(&self, f: F) {
        self.imm_queue.lock().unwrap().push(Box::new(f));
    }

    pub unsafe fn run(self: Arc<Self>) -> ! {
        GLOBAL_EPOLL.with(|x| {
            *x.borrow_mut() = Some(self.clone());
        });
        let mut events: Vec<EpollEvent> = vec![EpollEvent::default(); 32];
        loop {
            loop {
                let imm_queue =
                    ::std::mem::replace(&mut *self.imm_queue.lock().unwrap(), Vec::new());
                if imm_queue.len() == 0 {
                    break;
                }
                for f in imm_queue {
                    f();
                }
            }
            let events_len = events.len();
            let n_ready = _epoll_wait(self.fd, events.as_mut_ptr(), events_len, -1);
            assert!(n_ready >= 0);
            /*if n_ready > 1 {
                println!("n_ready = {}", n_ready);
            }*/
            for ev in &events[..n_ready as usize] {
                if ev.events & (EPOLLIN | EPOLLOUT) != 0 {
                    //println!("Free event {:x} {:?}", ev.events, ev.data as usize as *mut AsyncState);
                    let mut state = Box::from_raw(ev.data as usize as *mut AsyncState);
                    (state.callback.take().unwrap())();
                    put_async_state(state);
                //println!("After callback");
                } else {
                    println!("unknown event(s): 0x{:x}", ev.events);
                }
            }
        }
    }
}

impl Drop for Epoll {
    fn drop(&mut self) {
        unsafe {
            File::from_raw_fd(self.fd as _);
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub enum EpollDirection {
    In,
    Out,
}

fn get_async_state() -> Box<AsyncState> {
    ASYNC_STATE_POOL.with(|pool| {
        pool.borrow_mut()
            .pop()
            .unwrap_or_else(|| Box::new(AsyncState::default()))
    })
}

fn put_async_state(mut x: Box<AsyncState>) {
    x.callback = None;
    x._epoll = None;
    ASYNC_STATE_POOL.with(|pool| pool.borrow_mut().push(x));
}

pub fn schedule<F: FnOnce() + 'static>(f: F) {
    //println!("schedule");
    let epoll = GLOBAL_EPOLL.with(|x| x.borrow().as_ref().unwrap().clone());
    epoll.schedule(f);
}

fn get_async_io_payload<
    T: 'static,
    P: FnMut(i32) -> Result<T, i32> + 'static,
    F: FnOnce(Result<T, i32>) + 'static,
>(
    epoll: Arc<Epoll>,
    fd: i32,
    direction: EpollDirection,
    poll_action: P,
    on_ready: F,
) -> Box<FnOnce()> {
    __get_async_io_payload(epoll, fd, direction, poll_action, on_ready, false)
}

fn __get_async_io_payload<
    T: 'static,
    P: FnMut(i32) -> Result<T, i32> + 'static,
    F: FnOnce(Result<T, i32>) + 'static,
>(
    epoll: Arc<Epoll>,
    fd: i32,
    direction: EpollDirection,
    mut poll_action: P,
    on_ready: F,
    registered: bool,
) -> Box<FnOnce()> {
    let epfd = epoll.fd;
    Box::new(move || {
        //println!("async io payload");
        let ret = poll_action(fd);
        //println!("async io payload (after poll_action)");
        match ret {
            Err(x) if x == -EAGAIN || x == -EWOULDBLOCK => {
                let mut state = get_async_state();
                state.callback = Some(__get_async_io_payload(
                    epoll.clone(),
                    fd,
                    direction,
                    poll_action,
                    on_ready,
                    true,
                ));
                state._epoll = Some(epoll);
                let direction_flag = match direction {
                    EpollDirection::In => EPOLLIN,
                    EpollDirection::Out => EPOLLOUT,
                };
                let ev = EpollEvent {
                    events: direction_flag | EPOLLET | EPOLLONESHOT,
                    data: Box::into_raw(state) as usize as _,
                };
                //println!("Alloc event {:?}", ev.data as usize as *mut AsyncState);
                let ret = unsafe { _epoll_ctl(epfd, EPOLL_CTL_ADD, fd, &ev) };
                assert!(ret >= 0);
            }
            x => {
                if registered {
                    assert!(
                        unsafe { _epoll_ctl(epfd, EPOLL_CTL_DEL, fd, ::std::ptr::null(),) } >= 0
                    );
                }
                on_ready(x); // fast path
            }
        }
    })
}

#[repr(C)]
#[derive(Copy, Clone)]
struct SockaddrIn {
    sin_family: u16, // e.g. AF_INET
    sin_port: u16,   // e.g. htons(3490)
    sin_addr: InAddr,
    sin_zero: [u8; 8],
}

#[repr(C)]
#[derive(Copy, Clone)]
struct InAddr {
    s_addr: u32,
}

#[repr(C, packed)]
#[derive(Copy, Clone, Default)]
struct EpollEvent {
    events: u32,
    data: u64,
}

fn invert_byteorder_u16(x: u16) -> u16 {
    unsafe {
        use std::mem::transmute;
        let buf: [u8; 2] = transmute(x);
        let out: [u8; 2] = [buf[1], buf[0]];
        transmute(out)
    }
}

#[derive(Debug)]
pub enum SocketError {
    AddrParse(AddrParseError),
    SocketCreate,
    Bind,
    Listen,
    Accept,
    Message(String),
}

pub struct Tcp4Listener {
    _addr: Ipv4Addr,
    _port: u16,
    fd: i32,
}

impl Tcp4Listener {
    pub fn new<A: AsRef<str>>(
        addr: A,
        port: u16,
        backlog: u32,
    ) -> Result<Tcp4Listener, SocketError> {
        let addr: Ipv4Addr = addr.as_ref().parse().map_err(SocketError::AddrParse)?;
        let sa = SockaddrIn {
            sin_family: AF_INET as _,
            sin_port: invert_byteorder_u16(port),
            sin_addr: InAddr {
                s_addr: unsafe { ::std::mem::transmute(addr.octets()) },
            },
            sin_zero: [0; 8],
        };
        let fd = unsafe { _socket(AF_INET, SOCK_STREAM, 0) };
        if fd < 0 {
            return Err(SocketError::SocketCreate);
        }
        if unsafe { _bind(fd, &sa, ::std::mem::size_of::<SockaddrIn>()) } < 0 {
            return Err(SocketError::Bind);
        }
        if unsafe { _listen(fd, backlog as _) } < 0 {
            return Err(SocketError::Listen);
        }

        unsafe {
            let mut socket_flags = _fcntl(fd, F_GETFL, 0) as u32;
            socket_flags |= O_NONBLOCK;
            assert!(_fcntl(fd, F_SETFL, socket_flags) >= 0);
        }

        Ok(Tcp4Listener {
            _addr: addr,
            _port: port,
            fd: fd,
        })
    }

    pub fn accept_async<F: Fn(Result<Arc<TcpStream>, i32>) -> Result<(), ()> + 'static>(
        self: Arc<Self>,
        ep: Arc<Epoll>,
        cb: F,
    ) {
        let ep2 = ep.clone();
        (get_async_io_payload(
            ep.clone(),
            self.fd,
            EpollDirection::In,
            move |fd| -> Result<Arc<TcpStream>, i32> {
                let mut incoming_sa: SockaddrIn = unsafe { ::std::mem::uninitialized() };
                let mut real_len: usize = ::std::mem::size_of::<SockaddrIn>();
                let conn = unsafe { _accept4(fd, &mut incoming_sa, &mut real_len, O_NONBLOCK) };
                if conn >= 0 {
                    unsafe {
                        let mut socket_flags = _fcntl(conn, F_GETFL, 0) as u32;
                        socket_flags |= O_NONBLOCK;
                        assert!(_fcntl(conn, F_SETFL, socket_flags) >= 0);
                    }
                    Ok(Arc::new(TcpStream {
                        fd: conn,
                        epoll: ep.clone(),
                    }))
                } else {
                    Err(conn)
                }
            },
            move |x| {
                schedule(|| {
                    if let Ok(()) = cb(x) {
                        self.accept_async(ep2, cb);
                    }
                });
            },
        ))();
    }
}

pub struct TcpStream {
    fd: i32,
    epoll: Arc<Epoll>,
}

impl TcpStream {
    pub fn __write_async(
        self: Arc<Self>,
        data: Vec<u8>,
        offset: usize,
        cb: impl FnOnce(Result<(usize, Vec<u8>), i32>) + 'static,
    ) {
        let mut data = Some(data);

        (get_async_io_payload(
            self.epoll.clone(),
            self.fd,
            EpollDirection::Out,
            move |fd| -> Result<(usize, Vec<u8>), i32> {
                let _data = data.as_ref().unwrap();
                let _data = &_data[offset..];
                let ret =
                    unsafe { _sendto(fd, _data.as_ptr(), _data.len(), 0, ::std::ptr::null(), 0) };
                if ret >= 0 {
                    Ok((ret as usize, data.take().unwrap()))
                } else {
                    Err(ret)
                }
            },
            move |x| {
                drop(self);
                cb(x);
            },
        ))();
    }

    pub fn write_async(
        self: Arc<Self>,
        data: Vec<u8>,
        cb: impl FnOnce(Result<(usize, Vec<u8>), i32>) + 'static,
    ) {
        self.__write_async(data, 0, cb)
    }

    pub fn write_all_async(
        self: Arc<Self>,
        data: Vec<u8>,
        cb: impl FnOnce(Result<Vec<u8>, i32>) + 'static,
    ) {
        fn inner(
            me: Arc<TcpStream>,
            data: Vec<u8>,
            offset: usize,
            cb: impl FnOnce(Result<Vec<u8>, i32>) + 'static,
        ) {
            let me2 = me.clone();
            me.__write_async(data, offset, move |result| match result {
                Ok((len, data)) => {
                    let new_offset = offset + len;
                    if new_offset == data.len() {
                        cb(Ok(data));
                    } else {
                        inner(me2, data, new_offset, cb);
                    }
                }
                Err(code) => {
                    cb(Err(code));
                }
            })
        }
        inner(self, data, 0, cb);
    }

    pub fn read_async(
        self: Arc<Self>,
        out: Vec<u8>,
        cb: impl FnOnce(Result<Vec<u8>, i32>) + 'static,
    ) {
        let mut out = Some(out);
        (get_async_io_payload(
            self.epoll.clone(),
            self.fd,
            EpollDirection::In,
            move |fd| -> Result<Vec<u8>, i32> {
                let _out = out.as_mut().unwrap();
                let out_cap = _out.capacity();
                let ret = unsafe {
                    _recvfrom(
                        fd,
                        _out.as_mut_ptr(),
                        out_cap,
                        0,
                        ::std::ptr::null_mut(),
                        ::std::ptr::null_mut(),
                    )
                };
                if ret >= 0 {
                    assert!(ret as usize <= out_cap);
                    unsafe {
                        _out.set_len(ret as usize);
                    }
                    Ok(out.take().unwrap())
                } else {
                    Err(ret)
                }
            },
            move |x| {
                drop(self);
                cb(x);
            },
        ))();
    }
}

impl Drop for TcpStream {
    fn drop(&mut self) {
        unsafe {
            File::from_raw_fd(self.fd as _);
        }
    }
}
