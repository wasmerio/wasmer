# In-runtime switch

Share one `Switch` across Machines and give each its own `Net`:

```rust,ignore
let switch = wasixv2_vnet_switch::Switch::new(Default::default())?;
let left = switch.network("192.0.2.10".parse()?, 24)?;
let right = switch.network("192.0.2.11".parse()?, 24)?;
// Pass each Net to its MachineBuilder::network (browser::builder also accepts it).
let listeners = switch.listeners(); // read-only, reachable TCP listeners + queue room
```

Each helper-created port has one unicast IPv4 or IPv6 address and a prefix route;
peers must be reachable through the configured routes. There is no implicit
external gateway or DNS provider. Loopback belongs to each namespace. IPv4-mapped
address owners are rejected; guest mapped-address handling remains vnet's.

Machine peers use the optional Backend peer-route hook and vnet's existing
cross-namespace socket path. Source ports are the guest's, not translated host
ports. Per-Net bind conflicts, reuse groups, accept/SYN queues, UDP filtering,
ICMP/error queues and TCP urgent/FIN/reset behavior are unchanged. Dropping the
Net and its last sockets releases its weak registration. No Rust Net/kernel is
shared between Machines, and no browser host socket is created.

The Backend also implements explicit stream/datagram publication and direct
backend clients, using vnet MemoryStream/Conn and bounded UDP queues. Those
host-facing clients have backend-owned ephemeral source ports, as the host
backend does. Config limits bound these explicit endpoints; Machine peer sockets
use their namespace/kernel socket limits and bounded vnet buffers. Published UDP
endpoints drop arrivals until attached and reset receive/error/peer state when
ownership changes. Publication handles keep resolved addresses alive.

`listeners()` is a snapshot, not a readiness reservation or event subscription.
It includes full queues, omits loopback, nonlocal freebind and wrong-family binds,
and stops exposing an address removed from the uplink. A connect must still
handle a listener closing or its queue filling after discovery. SDK
wait/expose/onListen wrappers and service-worker HTTP framing belong in SDK.

Native integration tests cover IPv4/IPv6, exact source tuples, same guest port in
separate Machines, loopback isolation, live discovery/reuseport, queue saturation,
publication attachment/lifetime, bounded stream/UDP buffers, urgent marks,
reset/EOF and connected UDP refusal/RECVERR. B8/B11 source fixtures exercise two
Machines over the same switch in Chromium and Firefox with JSPI removed.

Linux authorities: net/ipv4/tcp.c:tcp_poll,tcp_close,tcp_shutdown;
net/ipv4/tcp_ipv4.c:tcp_v4_connect,tcp_v4_syn_recv_sock;
net/ipv4/udp.c:udp_sendmsg,udp_recvmsg,__udp4_lib_err (now udp_err).
