#[cfg(feature = "proto-ipv4")]
mod ipv4;
#[cfg(feature = "proto-ipv6")]
mod ipv6;
#[cfg(feature = "proto-sixlowpan")]
mod sixlowpan;

#[allow(unused)]
use std::vec::Vec;

use crate::tests::setup;

use rstest::*;

use super::*;

use crate::iface::Interface;
use crate::phy::ChecksumCapabilities;
#[cfg(feature = "alloc")]
use crate::phy::Loopback;
use crate::time::Instant;

#[allow(unused)]
fn fill_slice(s: &mut [u8], val: u8) {
    for x in s.iter_mut() {
        *x = val
    }
}

#[allow(unused)]
fn recv_all(device: &mut crate::tests::TestingDevice, timestamp: Instant) -> Vec<Vec<u8>> {
    let mut pkts = Vec::new();
    while let Some(pkt) = device.tx_queue.pop_front() {
        pkts.push(pkt)
    }
    pkts
}

#[derive(Debug, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
struct MockTxToken;

impl TxToken for MockTxToken {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut junk = [0; 1536];
        f(&mut junk[..len])
    }
}

#[test]
#[should_panic(expected = "The hardware address does not match the medium of the interface.")]
#[cfg(all(feature = "medium-ip", feature = "medium-ethernet", feature = "alloc"))]
fn test_new_panic() {
    let mut device = Loopback::new(Medium::Ethernet);
    let config = Config::new(HardwareAddress::Ip);
    Interface::new(config, &mut device, Instant::ZERO);
}

#[cfg(feature = "socket-udp")]
#[rstest]
#[case::ip(Medium::Ip)]
#[cfg(feature = "medium-ip")]
#[case::ethernet(Medium::Ethernet)]
#[cfg(feature = "medium-ethernet")]
#[case::ieee802154(Medium::Ieee802154)]
#[cfg(feature = "medium-ieee802154")]
fn test_handle_udp_broadcast(#[case] medium: Medium) {
    use crate::socket::udp;
    use crate::wire::IpEndpoint;

    static UDP_PAYLOAD: [u8; 5] = [0x48, 0x65, 0x6c, 0x6c, 0x6f];

    let (mut iface, mut sockets, _device) = setup(medium);

    let rx_buffer = udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY], vec![0; 15]);
    let tx_buffer = udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY], vec![0; 15]);

    let udp_socket = udp::Socket::new(rx_buffer, tx_buffer);

    let mut udp_bytes = vec![0u8; 13];
    let mut packet = UdpPacket::new_unchecked(&mut udp_bytes);

    let socket_handle = sockets.add(udp_socket);

    #[cfg(feature = "proto-ipv6")]
    let src_ip = Ipv6Address::new(0xfe80, 0, 0, 0, 0, 0, 0, 1);
    #[cfg(all(not(feature = "proto-ipv6"), feature = "proto-ipv4"))]
    let src_ip = Ipv4Address::new(0x7f, 0x00, 0x00, 0x02);

    let udp_repr = UdpRepr {
        src_port: 67,
        dst_port: 68,
    };

    #[cfg(feature = "proto-ipv6")]
    let ip_repr = IpRepr::Ipv6(Ipv6Repr {
        src_addr: src_ip,
        dst_addr: IPV6_LINK_LOCAL_ALL_NODES,
        next_header: IpProtocol::Udp,
        payload_len: udp_repr.header_len() + UDP_PAYLOAD.len(),
        hop_limit: 0x40,
    });
    #[cfg(all(not(feature = "proto-ipv6"), feature = "proto-ipv4"))]
    let ip_repr = IpRepr::Ipv4(Ipv4Repr {
        src_addr: src_ip,
        dst_addr: Ipv4Address::BROADCAST,
        next_header: IpProtocol::Udp,
        payload_len: udp_repr.header_len() + UDP_PAYLOAD.len(),
        hop_limit: 0x40,
    });
    let dst_addr = ip_repr.dst_addr();

    // Bind the socket to port 68
    let socket = sockets.get_mut::<udp::Socket>(socket_handle);
    assert_eq!(socket.bind(68), Ok(()));
    assert!(!socket.can_recv());
    assert!(socket.can_send());

    udp_repr.emit(
        &mut packet,
        &ip_repr.src_addr(),
        &ip_repr.dst_addr(),
        UDP_PAYLOAD.len(),
        |buf| buf.copy_from_slice(&UDP_PAYLOAD),
        &ChecksumCapabilities::default(),
    );

    // Packet should be handled by bound UDP socket
    assert_eq!(
        iface.inner.process_udp(
            &mut sockets,
            PacketMeta::default(),
            false,
            ip_repr,
            packet.into_inner(),
        ),
        None
    );

    // Make sure the payload to the UDP packet processed by process_udp is
    // appended to the bound sockets rx_buffer
    let socket = sockets.get_mut::<udp::Socket>(socket_handle);
    assert!(socket.can_recv());
    assert_eq!(
        socket.recv(),
        Ok((
            &UDP_PAYLOAD[..],
            udp::UdpMetadata {
                local_address: Some(dst_addr),
                ..IpEndpoint::new(src_ip.into(), 67).into()
            }
        ))
    );
}

#[test]
#[cfg(all(feature = "medium-ip", feature = "socket-tcp", feature = "proto-ipv6"))]
pub fn tcp_not_accepted() {
    let (mut iface, mut sockets, _) = setup(Medium::Ip);
    let tcp = TcpRepr {
        src_port: 4242,
        dst_port: 4243,
        control: TcpControl::Syn,
        seq_number: TcpSeqNumber(-10001),
        ack_number: None,
        window_len: 256,
        window_scale: None,
        max_seg_size: None,
        sack_permitted: false,
        sack_ranges: [None, None, None],
        timestamp: None,
        payload: &[],
    };

    let mut tcp_bytes = vec![0u8; tcp.buffer_len()];

    tcp.emit(
        &mut TcpPacket::new_unchecked(&mut tcp_bytes),
        &Ipv6Address::new(0xfe80, 0, 0, 0, 0, 0, 0, 2).into(),
        &Ipv6Address::new(0xfe80, 0, 0, 0, 0, 0, 0, 1).into(),
        &ChecksumCapabilities::default(),
    );

    assert_eq!(
        iface.inner.process_tcp(
            &mut sockets,
            false,
            IpRepr::Ipv6(Ipv6Repr {
                src_addr: Ipv6Address::new(0xfe80, 0, 0, 0, 0, 0, 0, 2),
                dst_addr: Ipv6Address::new(0xfe80, 0, 0, 0, 0, 0, 0, 1),
                next_header: IpProtocol::Tcp,
                payload_len: tcp.buffer_len(),
                hop_limit: 64,
            }),
            &tcp_bytes,
        ),
        Some(Packet::new_ipv6(
            Ipv6Repr {
                src_addr: Ipv6Address::new(0xfe80, 0, 0, 0, 0, 0, 0, 1),
                dst_addr: Ipv6Address::new(0xfe80, 0, 0, 0, 0, 0, 0, 2),
                next_header: IpProtocol::Tcp,
                payload_len: tcp.buffer_len(),
                hop_limit: 64,
            },
            IpPayload::Tcp(TcpRepr {
                src_port: 4243,
                dst_port: 4242,
                control: TcpControl::Rst,
                seq_number: TcpSeqNumber(0),
                ack_number: Some(TcpSeqNumber(-10000)),
                window_len: 0,
                window_scale: None,
                max_seg_size: None,
                sack_permitted: false,
                sack_ranges: [None, None, None],
                timestamp: None,
                payload: &[],
            })
        ))
    );
    // Unspecified destination address.
    tcp.emit(
        &mut TcpPacket::new_unchecked(&mut tcp_bytes),
        &Ipv6Address::new(0xfe80, 0, 0, 0, 0, 0, 0, 2).into(),
        &Ipv6Address::UNSPECIFIED.into(),
        &ChecksumCapabilities::default(),
    );

    assert_eq!(
        iface.inner.process_tcp(
            &mut sockets,
            false,
            IpRepr::Ipv6(Ipv6Repr {
                src_addr: Ipv6Address::new(0xfe80, 0, 0, 0, 0, 0, 0, 2),
                dst_addr: Ipv6Address::UNSPECIFIED,
                next_header: IpProtocol::Tcp,
                payload_len: tcp.buffer_len(),
                hop_limit: 64,
            }),
            &tcp_bytes,
        ),
        None,
    );
}

#[test]
#[cfg(all(feature = "medium-ip", feature = "socket-tcp", feature = "proto-ipv4"))]
pub fn tcp_listen_drops_unspecified_src() {
    use crate::socket::tcp;

    let (mut iface, mut sockets, _) = setup(Medium::Ip);

    let tcp_socket = tcp::Socket::new(
        tcp::SocketBuffer::new(vec![0; 64]),
        tcp::SocketBuffer::new(vec![0; 64]),
    );
    let handle = sockets.add(tcp_socket);
    sockets.get_mut::<tcp::Socket>(handle).listen(1234).unwrap();

    let tcp = TcpRepr {
        src_port: 65000,
        dst_port: 1234,
        control: TcpControl::Syn,
        seq_number: TcpSeqNumber(0),
        ack_number: None,
        window_len: 1024,
        window_scale: None,
        max_seg_size: Some(1460),
        sack_permitted: false,
        sack_ranges: [None, None, None],
        timestamp: None,
        payload: &[],
    };

    let mut tcp_bytes = vec![0u8; tcp.buffer_len()];
    tcp.emit(
        &mut TcpPacket::new_unchecked(&mut tcp_bytes),
        &Ipv4Address::UNSPECIFIED.into(),
        &Ipv4Address::new(127, 0, 0, 1).into(),
        &ChecksumCapabilities::default(),
    );

    let reply = iface.inner.process_tcp(
        &mut sockets,
        false,
        IpRepr::Ipv4(Ipv4Repr {
            src_addr: Ipv4Address::UNSPECIFIED,
            dst_addr: Ipv4Address::new(127, 0, 0, 1),
            next_header: IpProtocol::Tcp,
            payload_len: tcp.buffer_len(),
            hop_limit: 64,
        }),
        &tcp_bytes,
    );

    assert_eq!(reply, None);
    assert!(sockets.get_mut::<tcp::Socket>(handle).is_listening());
}

/// A UDP datagram's packet ID reaches the device over IPv6, as it does over IPv4.
#[test]
#[cfg(all(
    feature = "medium-ip",
    feature = "socket-udp",
    feature = "proto-ipv6",
    feature = "packetmeta-id"
))]
fn udp_packet_id_reaches_the_device_over_ipv6() {
    use crate::socket::udp;

    let (mut iface, mut sockets, mut device) = setup(Medium::Ip);
    let mut socket = udp::Socket::new(
        udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY], vec![0; 64]),
        udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY], vec![0; 64]),
    );
    socket.bind(4243).unwrap();
    let mut meta = udp::UdpMetadata::from((Ipv6Address::new(0xfe80, 0, 0, 0, 0, 0, 0, 2), 4242));
    meta.meta.id = 7;
    socket.send_slice(b"id", meta).unwrap();
    sockets.add(socket);

    iface.poll(Instant::from_millis(0), &mut device, &mut sockets);
    assert_eq!(device.tx_queue.len(), 1);
    assert_eq!(device.tx_metas, [meta.meta]);
}

/// A TCP segment the device reports lost goes out again at once, from the same sequence
/// number, without waiting for duplicate ACKs or the retransmission timer.
#[test]
#[cfg(all(
    feature = "medium-ip",
    feature = "socket-tcp",
    feature = "proto-ipv6",
    feature = "local-loss"
))]
fn tcp_segment_lost_locally_is_sent_again() {
    let (mut iface, mut sockets, mut device) = setup(Medium::Ip);
    let local = Ipv6Address::new(0xfe80, 0, 0, 0, 0, 0, 0, 1);
    let remote = Ipv6Address::new(0xfe80, 0, 0, 0, 0, 0, 0, 2);
    let socket = crate::socket::tcp::Socket::new(
        crate::socket::tcp::SocketBuffer::new(vec![0; 256]),
        crate::socket::tcp::SocketBuffer::new(vec![0; 256]),
    );
    let handle = sockets.add(socket);
    sockets
        .get_mut::<crate::socket::tcp::Socket>(handle)
        .connect(iface.context(), (remote, 4242), 4243)
        .unwrap();

    // Each sent packet as its TCP sequence number and payload length.
    let segments = |device: &mut crate::tests::TestingDevice| -> Vec<(TcpSeqNumber, usize)> {
        device
            .tx_queue
            .drain(..)
            .map(|p| {
                let ip = Ipv6Packet::new_checked(&p[..]).unwrap();
                let tcp = TcpPacket::new_checked(ip.payload()).unwrap();
                (tcp.seq_number(), tcp.payload().len())
            })
            .collect()
    };

    iface.poll(Instant::from_millis(0), &mut device, &mut sockets);
    let syn = segments(&mut device);
    assert_eq!(syn.len(), 1);

    // The SYN-ACK.
    let reply = TcpRepr {
        src_port: 4242,
        dst_port: 4243,
        control: TcpControl::Syn,
        seq_number: TcpSeqNumber(5000),
        ack_number: Some(syn[0].0 + 1),
        window_len: 256,
        window_scale: None,
        max_seg_size: Some(1000),
        sack_permitted: false,
        sack_ranges: [None, None, None],
        timestamp: None,
        payload: &[],
    };
    let ip = Ipv6Repr {
        src_addr: remote,
        dst_addr: local,
        next_header: IpProtocol::Tcp,
        payload_len: reply.buffer_len(),
        hop_limit: 64,
    };
    let mut bytes = vec![0u8; ip.buffer_len() + reply.buffer_len()];
    ip.emit(&mut Ipv6Packet::new_unchecked(&mut bytes[..]));
    reply.emit(
        &mut TcpPacket::new_unchecked(&mut bytes[ip.buffer_len()..]),
        &remote.into(),
        &local.into(),
        &ChecksumCapabilities::default(),
    );
    device.rx_queue.push_back(bytes);
    iface.poll(Instant::from_millis(10), &mut device, &mut sockets);
    segments(&mut device);
    device.tx_metas.clear();

    sockets
        .get_mut::<crate::socket::tcp::Socket>(handle)
        .send_slice(b"local loss")
        .unwrap();
    iface.poll(Instant::from_millis(20), &mut device, &mut sockets);
    let sent = segments(&mut device);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].1, 10);
    let meta = device.tx_metas[0];
    assert_eq!(meta.transport, Some(IpProtocol::Tcp));

    device.losses.push_back(crate::phy::LocalLoss {
        transport: IpProtocol::Tcp,
        id: meta.id,
        reason: crate::phy::LossReason::NoAck,
    });
    iface.poll(Instant::from_millis(30), &mut device, &mut sockets);
    assert_eq!(segments(&mut device), sent);

    // A second report of the same segment is ignored: its tag was taken.
    device.losses.push_back(crate::phy::LocalLoss {
        transport: IpProtocol::Tcp,
        id: meta.id,
        reason: crate::phy::LossReason::NoAck,
    });
    iface.poll(Instant::from_millis(40), &mut device, &mut sockets);
    assert_eq!(segments(&mut device), vec![]);
}
