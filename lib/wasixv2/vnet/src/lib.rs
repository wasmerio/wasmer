//! Socket-level network, one per network namespace: port tables, interfaces and routes,
//! in-memory streams and datagram queues between local sockets, publishing, and the trait that
//! backends implement for traffic leaving the namespace. There is no packet-level stack.
