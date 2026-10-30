//! Wire compatibility checked against the v1 reference record definitions, development only.
#![cfg(not(target_arch = "wasm32"))]
use std::{net::SocketAddr, time::Duration};
use virtual_net::{
    NetworkError,
    meta::{
        MessageRequest as Request, MessageResponse as Response, RequestType as Op,
        ResponseType as Reply, SocketId,
    },
};
use wasixv2_abi::Errno;
use wasixv2_vnet::{Answer, Lookup, Shutdown};
use wasixv2_vnet_web::{Command, Event, codec};
fn addr() -> SocketAddr {
    "[2001:db8::1]:1234".parse().unwrap()
}
fn packed<T: serde::Serialize>(value: T) -> Vec<u8> {
    bincode::serde::encode_to_vec(value, bincode::config::legacy()).unwrap()
}
#[test]
fn requests_match_reference_protocol_including_ipv6_and_shutdown() {
    let peer = addr();
    let socket = SocketId::from(9);
    let cases = vec![
        (
            Command::Connect {
                request: 7,
                socket: 9,
                peer,
            },
            Request::Interface {
                req: Op::ConnectTcp {
                    socket_id: socket,
                    addr: "[::]:0".parse().unwrap(),
                    peer,
                },
                req_id: Some(7),
            },
        ),
        (
            Command::Bind {
                request: 7,
                socket: 9,
                addr: peer,
            },
            Request::Interface {
                req: Op::BindUdp {
                    socket_id: socket,
                    addr: peer,
                    reuse_port: false,
                    reuse_addr: false,
                },
                req_id: Some(7),
            },
        ),
        (
            Command::Listen {
                request: 7,
                socket: 9,
                addr: peer,
            },
            Request::Interface {
                req: Op::ListenTcp {
                    socket_id: socket,
                    addr: peer,
                    only_v6: true,
                    reuse_port: false,
                    reuse_addr: false,
                },
                req_id: Some(7),
            },
        ),
        (
            Command::Lookup {
                request: 7,
                lookup: Lookup::Name("example.test".into()),
            },
            Request::Interface {
                req: Op::Resolve {
                    host: "example.test.".into(),
                    port: None,
                    dns_server: None,
                },
                req_id: Some(7),
            },
        ),
        (
            Command::Send {
                request: 7,
                socket: 9,
                data: b"abc".to_vec(),
            },
            Request::Send {
                socket,
                data: b"abc".to_vec(),
                req_id: Some(7),
            },
        ),
        (
            Command::SendTo {
                socket: 9,
                data: vec![],
                addr: peer,
            },
            Request::SendTo {
                socket,
                data: vec![],
                addr: peer,
                req_id: Some(0),
            },
        ),
        (
            Command::Close { socket: 9 },
            Request::Socket {
                socket,
                req: Op::Close,
                req_id: None,
            },
        ),
        (
            Command::Reset { socket: 9 },
            Request::Socket {
                socket,
                req: Op::SetLinger(Some(Duration::ZERO)),
                req_id: None,
            },
        ),
        (
            Command::Accept {
                socket: 9,
                child: 10,
            },
            Request::Socket {
                socket,
                req: Op::BeginAccept(10.into()),
                req_id: None,
            },
        ),
    ];
    for (command, expected) in cases {
        let bytes = codec::encode(&command).unwrap();
        assert_eq!(bytes, packed(&expected), "{command:?}");
        let (_, used): (Request, _) =
            bincode::serde::decode_from_slice(&bytes, bincode::config::legacy()).unwrap();
        assert_eq!(used, bytes.len());
    }
    for (how, reference) in [
        (Shutdown::Read, virtual_net::meta::Shutdown::Read),
        (Shutdown::Write, virtual_net::meta::Shutdown::Write),
        (Shutdown::Both, virtual_net::meta::Shutdown::Both),
    ] {
        assert_eq!(
            codec::encode(&Command::Shutdown {
                request: 7,
                socket: 9,
                how
            })
            .unwrap(),
            packed(Request::Socket {
                socket,
                req: Op::Shutdown(reference),
                req_id: Some(7)
            })
        );
    }
}
#[test]
fn responses_match_reference_protocol() {
    let socket_id = 9.into();
    let responses = [
        Response::ResponseToRequest {
            req_id: 7,
            res: Reply::None,
        },
        Response::ResponseToRequest {
            req_id: 7,
            res: Reply::Socket(socket_id),
        },
        Response::ResponseToRequest {
            req_id: 7,
            res: Reply::IpAddressList(vec![addr().ip()]),
        },
        Response::Recv {
            socket_id,
            data: b"abc".to_vec(),
        },
        Response::RecvWithAddr {
            socket_id,
            data: vec![],
            addr: addr(),
        },
        Response::Sent {
            socket_id,
            req_id: 7,
            amount: 2,
        },
        Response::SendError {
            socket_id,
            req_id: 7,
            error: NetworkError::ConnectionReset,
        },
        Response::FinishAccept {
            socket_id,
            child_id: 10.into(),
            addr: addr(),
        },
        Response::Closed { socket_id },
    ];
    for (index, response) in responses.into_iter().enumerate() {
        let event = codec::decode(&packed(response)).unwrap();
        match (index, event) {
            (
                0 | 1,
                Event::Complete {
                    request: 7,
                    result: Ok(()),
                },
            ) => {}
            (
                2,
                Event::Lookup {
                    request: 7,
                    result: Ok(found),
                },
            ) => assert!(
                matches!(found.answer, Answer::Addrs { addrs, .. } if addrs == [addr().ip()])
            ),
            (3, Event::Data { socket: 9, data }) => assert_eq!(data, b"abc"),
            (
                4,
                Event::Datagram {
                    socket: 9,
                    data,
                    addr: peer,
                },
            ) => {
                assert!(data.is_empty());
                assert_eq!(peer, addr());
            }
            (
                5,
                Event::Sent {
                    request: 7,
                    socket: 9,
                    amount: 2,
                },
            ) => {}
            (
                6,
                Event::SendError {
                    request: 7,
                    socket: 9,
                    error: Errno::ECONNRESET,
                },
            ) => {}
            (
                7,
                Event::Accepted {
                    socket: 9,
                    child: 10,
                    peer,
                },
            ) => assert_eq!(peer, addr()),
            (8, Event::Eof { socket: 9 }) => {}
            (index, event) => panic!("wrong response {index}: {event:?}"),
        }
    }
}
#[test]
fn errors_are_explicit_and_malformed_frames_are_rejected() {
    let cases = [
        (NetworkError::ConnectionRefused, Errno::ECONNREFUSED),
        (NetworkError::WouldBlock, Errno::EAGAIN),
        (NetworkError::MessageSize, Errno::EMSGSIZE),
        (NetworkError::HostUnreachable, Errno::EHOSTUNREACH),
        (NetworkError::PermissionDenied, Errno::EACCES),
    ];
    for (error, expected) in cases {
        assert!(
            matches!(codec::decode(&packed(Response::ResponseToRequest { req_id: 7, res: Reply::Err(error) })),
            Ok(Event::Complete { result: Err(actual), .. }) if actual == expected)
        );
    }
    let valid = packed(Response::Recv {
        socket_id: 9.into(),
        data: vec![1, 2, 3],
    });
    for length in 0..valid.len() {
        assert!(codec::decode(&valid[..length]).is_err());
    }
    let mut trailing = valid;
    trailing.push(0);
    assert!(codec::decode(&trailing).is_err());
    assert!(codec::decode(&[0; codec::MAX_FRAME + 1]).is_err());
    assert!(codec::decode(&packed((1u32, 9u64, u64::MAX))).is_err());
    assert!(codec::decode(&packed((0u32, 7u64, 1u32, 99u32))).is_err());
    assert!(
        codec::encode(&Command::Lookup {
            request: 1,
            lookup: Lookup::Addr(addr().ip())
        })
        .is_err()
    );
}
