// NOTE: not currently in use - but kept here to allow easy re-implementation.

use std::net::IpAddr;

use crate::config::WasmerEnv;

use super::AsyncCliCommand;

/// Connects to the Wasmer Edge distributed network.
#[derive(clap::Parser, Debug)]
pub struct CmdConnect {
    #[clap(flatten)]
    env: WasmerEnv,

    /// Runs in promiscuous mode
    #[clap(long)]
    pub promiscuous: bool,
    /// Prints the network token rather than connecting
    #[clap(long)]
    pub print: bool,
    /// Skips bringing the interface up using the `ip` tool.
    #[clap(long)]
    pub leave_down: bool,
    /// Do not modify the postfix of the URL before connecting
    #[clap(long)]
    pub leave_postfix: bool,
    /// One or more static IP address to assign the interface
    #[clap(long)]
    pub ip: Vec<IpAddr>,
    /// Thr URL we will be connecting to
    #[clap(index = 1)]
    pub url: url::Url,
}

#[async_trait::async_trait]
impl AsyncCliCommand for CmdConnect {
    type Output = ();

    // Note (xdoardo, 28 Oct 2024):
    // This part of the code is commented out as we did not manage
    // to implement tun-tap yet.
    //
    //
    //
    //#[cfg(all(target_os = "linux", feature = "tun-tap"))]
    //async fn run_async(mut self) -> Result<(), anyhow::Error> {
    //    use edge_schema::{AppId, NetworkIdEncodingMethod, WELL_KNOWN_VPN};
    //    use virtual_mio::Selector;

    //    use crate::net::TunTapSocket;

    //    // If the URL does not include the well known postfix then add it
    //    if !self.leave_postfix {
    //        self.url.set_path(WELL_KNOWN_VPN);
    //    }

    //    if self.print {
    //        println!("websocket-url: {}", self.url.as_str());
    //        return Ok(());
    //    }

    //    print!("Connecting...");
    //    let socket = TunTapSocket::create(
    //        Selector::new(),
    //        self.url.clone(),
    //        self.leave_down == false,
    //        self.ip,
    //    )
    //    .await
    //    .map_err(|err| {
    //        println!("failed");
    //        err
    //    })?;
    //    println!("\rConnected to {}    ", self.url.as_str());

    //    for cidr in socket.ips().iter() {
    //        println!("Your IP:  {}/{}", cidr.ip, cidr.prefix);
    //    }
    //    for route in socket.routes().iter() {
    //        println!(
    //            "Gateway: {}/{} -> {}",
    //            route.cidr.ip, route.cidr.prefix, route.via_router
    //        );
    //    }
    //    for cidr in socket.ips().iter() {
    //        if let Some((app_id, _)) =
    //            AppId::from_ip(&cidr.ip, NetworkIdEncodingMethod::PrivateProjection)
    //        {
    //            let ip = app_id.into_ip(
    //                cidr.ip,
    //                0x00_1001,
    //                NetworkIdEncodingMethod::PrivateProjection,
    //            );
    //            println!("Instance: {}/{}", ip, cidr.prefix);
    //        }
    //    }
    //    println!("Press ctrl-c to terminate");
    //    socket.await?;

    //    Ok(())
    //}

    //#[cfg(not(all(target_os = "linux", feature = "tun-tap")))]
    async fn run_async(self) -> Result<(), anyhow::Error> {
        Err(anyhow::anyhow!(
            "This CLI does not support the 'connect' command: only available on Linux (feature: tun-tap)"
        ))
    }
}
