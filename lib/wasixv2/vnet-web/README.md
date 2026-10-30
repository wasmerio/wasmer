# Browser relay backend

`WebBackend` implements vnet's existing `Backend`, `BackendStream` and
`BackendDatagram`. It owns no engine objects and changes no guest ABI or
kernel/exec contract. The kernel retains routes, namespaces, bind conflicts,
local sockets, DNS protocol handling and accept-queue policy.

Construct a backend and driver with explicit `Capabilities` and `Config` limits.
Run `Driver::run(transport)` on an independent I/O worker. A worker-local
`Transport` implements ordered command submission, event reception, independent
connection-health polling and idempotent close. It has no Send bound; JS objects
never enter the shared backend. Dropping the run future closes its transport,
fails pending operations and wakes the original guest wait queues. Manual
`Driver::poll` users must close their transport on cancellation themselves.

`Classic<T: FrameTransport>` translates whole binary websocket messages using the
v1 relay's bincode legacy tags and layouts. Native-only development dependencies
check these bytes against the reference types and exercise the actual
`RemoteNetworkingServer` over bounded framed channels and loopback TCP/UDP/DNS
on IPv4 and IPv6. Production dependencies contain no v1 runtime or Tokio.
The local relay fixture does not exercise a browser WebSocket implementation.

For WISP, the SDK's callback adapter implements `Transport` and uses
`Capabilities::wisp(ipv6)`: TCP and name resolution only, with no UDP or ingress.
The adapter translates successful sends to `Sent`, refused/failed operations to
explicit errno events, and provider EOF/reset to distinct events. Resolver
providers can opt into reverse and raw-forward DNS and return the corresponding
`Found` answer forms (for example through configured DoH). WISP protocol bytes
never go through the classic codec. This crate provides the provider contract;
JS callbacks, endpoint authentication/selection, CORS and worker bootstrap belong
to the embedding SDK/B2 integration.

Queue and lifecycle rules:

- Stream writes are partial and bounded. One send per flow is outstanding; a short
  `Sent` removes only its accepted prefix. EAGAIN waits for a provider `Writable`
  event, without busy retrying. FIN follows all accepted bytes.
- Receive queues are bounded. The driver retains at most one additional bounded
  relay frame while a reader is full, then resumes after a read wakes it. With the
  classic multiplexed protocol this backpressures subsequent frames for all flows;
  independent transport-health polling still detects disconnect. The JS adapter
  must also bound its own event queue and WebSocket `bufferedAmount`.
- UDP preserves empty messages, source addresses, peek and full lengths for
  truncation. Excess receive datagrams are dropped. Outgoing datagrams are bounded
  and released once the transport takes them; classic UDP has no success ACK.
- Dropping the last stream reference drains accepted writes before close, or aborts
  on unread data. Dropped publications close reserved and late-accepted children.
  Ingress leaves connections in the relay backlog while `poll_room` is Full;
  Gone accepts and resets, and Ready offers the connection to the guest. Sink
  calls and guest wakeups occur outside the backend mutex.
- Transport failure uses ECONNABORTED for affected sockets, not an invented peer
  reset. Reconnect accepts new sockets; old handles remain failed and no bytes are
  replayed. Pending resolver callbacks complete exactly once, outside locks.

## Protocol limits and follow-ups

The stock v1 relay declares `Closed` but currently does not send it for TCP EOF
(`server.rs:RemoteAdapterSocket::drain_reads_and_accepts`, `Ok(0)` branch). Its
wire format cannot distinguish FIN from a peer reset. Consequently its default
`eof` capability is false. The backend model handles FIN/reset correctly when a
relay/provider supplies them; full stock-relay half-close/reset parity requires
relay-side work. Do not advertise that parity from the native model alone.

Classic resolution returns addresses only: the driver uses the question as the
canonical name and a 30-second synthesized TTL. Requests use absolute names to
avoid the relay host's search list. Reverse and raw-forward DNS require a separate
resolver-capable provider; unsupported lookups fail promptly. The stock UDP relay
also drops host send errors, cannot connect its UDP socket for asynchronous ICMP
errors, and drops empty inbound UDP datagrams. The backend supports empty inbound
messages when supplied, but the stock relay needs fixes for those cases.

Classic TCP publication supports fixed addresses, accept credits and asynchronous
bind-error reporting through `take_publication_error`. It has no synchronous
port-zero address reply or configurable native listen-backlog record: port-zero
publication is explicitly unsupported, and guest backlog enforcement remains in
the sink. UDP publication and urgent data are unsupported. Socket options with no
implemented wire effect return EOPNOTSUPP. These are explicit provider limits,
not changes to guest Linux socket policy.

B2 must implement and browser-test a bounded worker-local WebSocket/provider
transport and cross-worker wakers. B8 owns facade/network injection. Real browser
worker, endpoint-replacement and WISP-provider integration tests remain with that
integration. Nothing here implements or depends on C1 or C2.

## Opt-in fidelity provider

`Capabilities::classic_fidelity(ipv6)` selects our additive versioned protocol:
FIN versus reset, UDP host-send errors, PTR/raw DNS, urgent byte marks and fixed
UDP publication. Use it only with a matching provider. The stock profile is
unchanged; port-zero publication remains unsupported. See
[RELAY-FIDELITY.md](RELAY-FIDELITY.md) for the byte contract, deterministic test
provider, Linux authorities, exact stock-server changes and remaining limits.
