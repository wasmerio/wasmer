# Classic relay fidelity v1

The production stock relay is `lib/virtual-net/src/server.rs`. B15 owns vnet-web
and its deterministic test relay, so the stock v1 server remains unchanged.
`Capabilities::classic` retains its limits. `classic_fidelity` must be selected
out of band only with a provider implementing the records below. This is a
host transport extension, never a guest ABI change. There is no negotiation or
fallback against old servers: their incompatible enum decoder would reject it.

Legacy requests/responses retain every existing tag/layout, including Closed
(response 6) meaning TCP FIN only and ReceivedFrom (2) preserving empty UDP.
New records use bincode legacy little endian fixed integers, u64 lengths/IDs,
and serde's binary IpAddr representation. Maximum whole message is 1 MiB;
unknown version/operation, trailing bytes and invalid errnos are protocol errors.

| Direction | Record, in wire order |
| --- | --- |
| request | u32(4), u32(version=1), u32(op=0), u64(request), u64(socket), Vec<u8>(urgent byte) |
| request | u32(4), u32(1), u32(op=1), u64(request), IpAddr(reverse address) |
| request | u32(4), u32(1), u32(op=2), u64(request), Vec<u8>(complete DNS query) |
| response | u32(7), u32(1), u32(op=0), u64(socket), i32(Linux reset errno) |
| response | u32(7), u32(1), u32(op=1), u64(socket), i32(Linux UDP send errno) |
| response | u32(7), u32(1), u32(op=2), u64(request), u32(TTL), u32(answer=0), String(PTR name) |
| response | u32(7), u32(1), u32(op=2), u64(request), u32(TTL), u32(answer=1), Vec<u8>(DNS response) |
| response | u32(7), u32(1), u32(op=3), u64(socket), Vec<u8>(last byte urgent) |

SendUrgent receives the ordinary Sent/SendError reply. The driver sends the
marked byte alone after acknowledgments for all preceding bytes; following data
waits for its acknowledgment. Incoming records may have a prefix before their
last urgent byte. Ordinary/urgent records share one ordered stream and bounded
receive slot. vnet::RecvQueue owns MSG_OOB, SO_OOBINLINE, SIOCATMARK and POLLPRI.

The test relay (`tests/wasixv2/browser/network-server.mjs`) uses real loopback
TCP/UDP services, deterministic A/AAAA/PTR/TXT answers, host UDP-send failures,
real fixed-port publications and a bounded Python urgent relay helper over real
localhost TCP sockets. The helper forwards MSG_OOB and receives inline with
SIOCATMARK before every one-byte read; both directions traverse native sockets.
The stock server patch below is still required for that deployment.

## Exact stock-server follow-up

1. Append the extension envelopes to MessageRequest/MessageResponse in
   `virtual-net/src/meta.rs`; never renumber existing variants. Require explicit
   fidelity configuration before accepting/emitting them. Gate the provider's
   advertised capabilities on this configuration.
2. In RemoteAdapterSocket::drain_reads_and_accepts, on TCP Ok(0), emit Closed
   once, disable further read polling and retain the writable half/socket until
   local close. On host ECONNRESET emit extension Reset(ECONNRESET), not Closed;
   preserve data records already read before the error. Other explicit host
   errors preserve their Linux errno. Transport loss remains ECONNABORTED in
   vnet-web. Authorities: Linux net/ipv4/tcp.c:tcp_recvmsg_locked,tcp_poll,
   tcp_shutdown,tcp_close; net/ipv4/tcp_input.c:tcp_reset.
3. In the UDP receive arm, emit ReceivedFrom also for Ok((0, source)); zero is
   a datagram length, never EOF (net/ipv4/udp.c:udp_recvmsg). In the SendTo arm,
   emit DatagramError for host send failures, even with reserved request ID 0;
   do not terminate the endpoint (udp_sendmsg). Connected-host ICMP/error-queue
   fidelity needs a further explicit connect/errqueue record; this profile does
   not claim that unsupported feature.
4. Serve reverse lookup with getnameinfo and raw-forward messages with the
   configured resolver, bounding packets to 65535 bytes and preserving DNS
   status/identifier/question. Return the exact extension answer/TTL; failures
   use the existing failed request response so pending callbacks finish once.
   Fixed deterministic answers in the test relay do not provide production DNS.
5. Use existing BindUdp for fixed host publication. Preserve source addresses and
   empty payloads, route replies through that same bound socket, close on the
   final endpoint/publication reference. Until attach, drop arrivals. Publication
   bind failures go to take_publication_error, never guest bind/listen.
6. Forward urgent writes with send(MSG_OOB), retaining EAGAIN until POLLOUT.
   For reads set SO_OOBINLINE on the host socket, watch POLLPRI, check SIOCATMARK
   before each read and split at the urgent byte. Emit one ordered Urgent record
   at that mark, ordinary Data elsewhere; bound every read/queue. Follow the
   existing vnet-host reactor implementation and Linux tcp_recv_urg,tcp_ioctl,
   tcp_poll,tcp_sendmsg_locked. Do not substitute ordinary writes for MSG_OOB.
7. Keep port-zero TCP/UDP publication unsupported. Publication::host_addr is
   synchronous while relay bind is asynchronous. Supporting it needs an explicit
   embedding API/bind-address completion design; returning port 0 as a resolved
   address would violate that contract. No change is made to kernel/exec.

Production native listen-backlog configuration, connected UDP ICMP/errqueue,
and a real WISP provider remain separate explicit limits.
