use std::{
    collections::{HashSet, VecDeque},
    io::{self, ErrorKind},
    mem::MaybeUninit,
    os::fd::{AsRawFd, RawFd},
    pin::Pin,
    process::Command,
    sync::Arc,
    task::{Context, Poll, Waker},
};

use anyhow::Context as _;
use bytes::Bytes;
use futures_util::{Future, StreamExt};
use mio::{event, unix::SourceFd, Interest, Registry, Token};
use parking_lot::Mutex;
use tun_tap::{Iface, Mode};
use virtual_mio::{InterestGuard, InterestHandler, InterestType, InterestWakerMap, Selector};
use virtual_net::{
    io_err_into_net_error, meta::FrameSerializationFormat, IpAddr, IpCidr, IpRoute, Ipv4Addr,
    Ipv6Addr, NetworkError, RemoteNetworkingClient, RemoteNetworkingClientDriver,
    VirtualNetworking, VirtualRawSocket,
};

fn cmd(cmd: &str, args: &[&str]) -> anyhow::Result<()> {
    let ecode = Command::new(cmd).args(args).spawn()?.wait()?;
    if ecode.success() {
        Ok(())
    } else {
        Err(anyhow::format_err!(
            "failed to execute linux command (cmd={}, args={:?}, code={:?})",
            cmd,
            args,
            ecode.code()
        ))
    }
}

pub struct TunTapSocket {}

impl TunTapSocket {
    pub async fn create(
        selector: Arc<Selector>,
        url: url::Url,
        bring_up: bool,
        static_ips: Vec<IpAddr>,
    ) -> anyhow::Result<TunTapDriver> {
        tracing::info!("creating tun/tap device");
        let iface = match Iface::without_packet_info("edge%d", Mode::Tun) {
            Ok(i) => i,
            Err(err) => {
                tracing::error!(
                    "This process does not have permissions to open TUN/TAP sockets - {err}"
                );
                return Err(err).context("failed to open TUN/TAP socket");
            }
        };

        // Create the remote client
        tracing::info!("connecting to {url}");
        let (stream, _res) = tokio_tungstenite::connect_async(url).await?;
        let (tx, rx) = stream.split();

        // Now pass it on to a remote networking adapter
        tracing::info!("established web socket connection to the edge");
        let (remote, mut remote_driver) =
            RemoteNetworkingClient::new_from_tokio_ws_io(tx, rx, FrameSerializationFormat::Bincode);

        tracing::info!("creating RAW socket");
        let mut client = tokio::select! {
            a = remote.bind_raw() => a,
            _ = Pin::new(&mut remote_driver) => {
                return Err(anyhow::format_err!("the driver closed before we could create the RAW socket"));
            }
        }?;

        // If we are to set a static IP address then do so
        for ip in static_ips {
            tracing::info!("setting static IP ({ip})");
            let res: anyhow::Result<()> = tokio::select! {
                a = async {
                    remote.ip_clear().await?;
                    remote.ip_add(ip, match ip.is_ipv4() {
                        true => 24,
                        false => 120,
                    }).await?;
                    Ok(())
                } => a,
                _ = Pin::new(&mut remote_driver) => {
                    return Err(anyhow::format_err!("the driver closed before it could set the static IP address"));
                }
            };
            res?;
        }

        // We were likely assigned a mac address and IP address, get them
        tracing::info!("getting remote socket addresses");
        let ips = tokio::select! {
            a = remote.ip_list() => a,
            _ = Pin::new(&mut remote_driver) => {
                return Err(anyhow::format_err!("the driver closed before we could the mac and IP addresses"));
            }
        }?;

        // Get the routes as well
        let routes = tokio::select! {
            a = remote.route_list() => a,
            _ = Pin::new(&mut remote_driver) => {
                return Err(anyhow::format_err!("the driver closed before we could retrieve the routes"));
            }
        }?;

        if bring_up {
            tracing::info!("bringing up device ({})", iface.name());
            interfaces::Interface::get_by_name(iface.name())?
                .ok_or_else(|| anyhow::format_err!("The TUN/TAP interface could not be found"))?
                .set_up(true)?;
            //cmd("ip", &["link", "set", "up", "dev", iface.name()])?;

            for ip in ips.iter() {
                let ip_str = ip.ip.to_string();
                let prefix = ip.prefix;
                let ip_cidr = format!("{ip_str}/{prefix}");

                println!("\rAssigning IP address ({ip_cidr})");
                if cmd(
                    "ip",
                    &["address", "add", ip_cidr.as_str(), "dev", iface.name()],
                )
                .is_err()
                {
                    println!(
                        "\rEscalating to elevated rights (sudo) due to limited user permissions"
                    );
                    cmd(
                        "sudo",
                        &[
                            "ip",
                            "address",
                            "add",
                            ip_cidr.as_str(),
                            "dev",
                            iface.name(),
                        ],
                    )?;
                }
            }

            for mut route in routes.iter().cloned() {
                // We change the addresses to some hard coded numbers that allow the routing to work properly
                route.cidr.prefix = 8;
                if route.cidr.ip.is_ipv6() {
                    route.cidr.ip = IpAddr::V6(Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 0));
                } else {
                    route.cidr.ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 0));
                }

                let ip_str = route.cidr.ip.to_string();
                let prefix = route.cidr.prefix;
                let ip_cidr = format!("{ip_str}/{prefix}");
                let via_str = route.via_router.to_string();
                let via = via_str.to_string();

                println!("\rAssigning route ({ip_cidr})");
                if cmd(
                    "ip",
                    &[
                        "route",
                        "add",
                        ip_cidr.as_str(),
                        "via",
                        via.as_str(),
                        "dev",
                        iface.name(),
                    ],
                )
                .is_err()
                {
                    println!(
                        "\rEscalating to elevated rights (sudo) due to limited user permissions"
                    );
                    cmd(
                        "sudo",
                        &[
                            "ip",
                            "route",
                            "add",
                            ip_cidr.as_str(),
                            "via",
                            via.as_str(),
                            "dev",
                            iface.name(),
                        ],
                    )?;
                }
            }
        }

        // Set non-blocking and wrap the MIO
        tracing::info!("switching to non-blocking IO");
        set_non_blocking(&iface)?;
        let handler = InterestWakerMap::default();
        let mut source = MioWrapper {
            fd: iface.as_raw_fd(),
        };

        // Register interest in the read and write events
        let interest = InterestGuard::new(
            &selector,
            Box::new(handler.clone()),
            &mut source,
            mio::Interest::READABLE.add(mio::Interest::WRITABLE),
        )
        .map_err(io_err_into_net_error)?;

        // Set the client handler
        client.set_handler(Box::new(handler.clone()))?;

        let driver = TunTapDriver {
            iface,
            handler,
            send_queue: Default::default(),
            _interest: interest,
            client,
            ips,
            routes,
            remote,
            remote_driver,
        };

        Ok(driver)
    }
}

fn set_non_blocking(iface: &Iface) -> anyhow::Result<()> {
    let fd = iface.as_raw_fd();
    let mut nonblock: libc::c_int = 1;
    let result = unsafe { libc::ioctl(fd, libc::FIONBIO, &mut nonblock) };
    if result == -1 {
        Err(std::io::Error::last_os_error().into())
    } else {
        Ok(())
    }
}

#[derive(Debug, Default)]
struct TunTapHandlerInner {
    wakers: Vec<Waker>,
    set: HashSet<InterestType>,
}

#[derive(Debug, Clone, Default)]
struct TunTapHandler {
    inner: Arc<Mutex<TunTapHandlerInner>>,
}
impl InterestHandler for TunTapHandler {
    fn push_interest(&mut self, interest: InterestType) {
        let mut guard = self.inner.lock();
        guard.set.insert(interest);
        guard.wakers.drain(..).for_each(Waker::wake);
    }

    fn pop_interest(&mut self, interest: InterestType) -> bool {
        let mut guard = self.inner.lock();
        guard.set.remove(&interest)
    }

    fn has_interest(&self, interest: InterestType) -> bool {
        let guard = self.inner.lock();
        guard.set.contains(&interest)
    }
}

pub struct TunTapDriver {
    iface: Iface,
    handler: InterestWakerMap,
    send_queue: VecDeque<Bytes>,
    _interest: InterestGuard,
    client: Box<dyn VirtualRawSocket + Sync + 'static>,
    ips: Vec<IpCidr>,
    routes: Vec<IpRoute>,
    remote: RemoteNetworkingClient,
    remote_driver: RemoteNetworkingClientDriver,
}

impl TunTapDriver {
    pub fn client(&self) -> &RemoteNetworkingClient {
        &self.remote
    }

    pub fn ips(&self) -> &Vec<IpCidr> {
        &self.ips
    }

    pub fn routes(&self) -> &Vec<IpRoute> {
        &self.routes
    }
}

impl Future for TunTapDriver {
    type Output = io::Result<()>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // Run the remote networking
        match Pin::new(&mut self.remote_driver).poll(cx) {
            Poll::Ready(()) => return Poll::Ready(Ok(())),
            Poll::Pending => {}
        }

        // Add the waker before we drain all the events
        // we need to read and send
        self.handler
            .add(virtual_mio::InterestType::Readable, cx.waker());
        self.handler
            .add(virtual_mio::InterestType::Writable, cx.waker());

        // First we drain the packet
        while let Some(packet) = self.send_queue.pop_front() {
            if self.client.try_send(&packet).is_ok() == false {
                self.send_queue.push_front(packet);
                break;
            }
        }

        // Now we drain all the packets from the interface (but only if we have room)
        if self.send_queue.is_empty() {
            let mut chunk = [0u8; 65536];
            loop {
                match self.iface.recv(&mut chunk) {
                    Ok(0) => return Poll::Ready(Err(ErrorKind::ConnectionReset.into())),
                    Ok(data) => {
                        // Send the packet down the client
                        let packet = &chunk[..data];
                        if self.client.try_send(packet).is_ok() == false {
                            self.send_queue.push_back(Bytes::copy_from_slice(packet));
                            break;
                        }
                        continue;
                    }
                    Err(err) if err.kind() == ErrorKind::WouldBlock => {
                        break;
                    }
                    Err(err) => return Poll::Ready(Err(err)),
                };
            }
        }

        // The client interface expects
        //
        // UNSAFE: Drain the client interface method `try_recv` expects a MaybeUninit and
        //         and guarantees it will fill in a portion of the memory
        let mut chunk: [MaybeUninit<u8>; 65536] = unsafe { MaybeUninit::uninit().assume_init() };
        loop {
            let chunk_unsafe: &mut [MaybeUninit<u8>] = &mut chunk[..];
            match self.client.try_recv(chunk_unsafe) {
                Ok(0) => return Poll::Ready(Err(ErrorKind::ConnectionReset.into())),
                Ok(data) => {
                    let chunk_unsafe: &mut [u8] = unsafe { std::mem::transmute(chunk_unsafe) };
                    let packet = &chunk_unsafe[..data];

                    // Send the packet down the client
                    self.iface.send(packet).ok();
                    continue;
                }
                Err(NetworkError::WouldBlock) => {
                    break;
                }
                Err(err) => {
                    tracing::error!("packet recv error - {}", err);
                    return Poll::Ready(Err(ErrorKind::Other.into()));
                }
            }
        }

        // Wait for some more interest or something to send
        Poll::Pending
    }
}

struct MioWrapper {
    fd: RawFd,
}

impl event::Source for MioWrapper {
    fn register(
        &mut self,
        registry: &Registry,
        token: Token,
        interests: Interest,
    ) -> io::Result<()> {
        SourceFd(&self.fd).register(registry, token, interests)
    }

    fn reregister(
        &mut self,
        registry: &Registry,
        token: Token,
        interests: Interest,
    ) -> io::Result<()> {
        SourceFd(&self.fd).reregister(registry, token, interests)
    }

    fn deregister(&mut self, registry: &Registry) -> io::Result<()> {
        SourceFd(&self.fd).deregister(registry)
    }
}
