//! Backport coverage for upstream FIPS #176: replies must never dial.
use super::*;
use crate::node::wire::build_msg1;
use crate::transport::{PacketBuffer, ReceivedPacket};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::net::TcpListener;

#[derive(Debug)]
struct CountingResolver(AtomicUsize);

impl crate::host_network::HostNetwork for CountingResolver {
    fn protect_socket(&self, _: &socket2::SockRef<'_>) -> std::io::Result<()> {
        Ok(())
    }

    fn resolve<'a>(
        &'a self,
        _: &'a str,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = std::io::Result<Vec<std::net::SocketAddr>>>
                + Send
                + Sync
                + 'a,
        >,
    > {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(vec!["127.0.0.1:9".parse().unwrap()]) })
    }
}

fn incoming_msg1(receiver: &TestNode, remote: TransportAddr) -> ReceivedPacket {
    let sender = make_node();
    let identity = PeerIdentity::from_pubkey_full(receiver.node.identity.pubkey_full());
    let mut connection = PeerConnection::outbound(LinkId::new(1), identity, 0);
    let noise = connection
        .start_handshake(sender.identity.keypair(), sender.startup_epoch, 0)
        .unwrap();
    ReceivedPacket::with_timestamp(
        receiver.transport_id,
        remote,
        PacketBuffer::new(build_msg1(SessionIndex::new(77), &noise)),
        Node::now_ms(),
    )
}

#[tokio::test]
async fn missing_tcp_handshake_carrier_never_dials_the_sender() {
    let mut receiver = make_test_node_tcp().await;
    // A live listener makes an accidental dial observable without relying on
    // packet loss, kernel connect timeouts, or an unused-port race.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let remote = TransportAddr::from_string(&listener.local_addr().unwrap().to_string());
    let packet = incoming_msg1(&receiver, remote);

    // The packet was queued by a TCP connection that has since disappeared.
    tokio::time::timeout(
        Duration::from_millis(250),
        receiver.node.handle_msg1(packet),
    )
    .await
    .expect("a missing reply carrier must not stall the node");
    assert_eq!(
        receiver.node.peer_count(),
        1,
        "the valid Msg1 must be processed"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(50), listener.accept())
            .await
            .is_err(),
        "a handshake reply must not reconnect to the sender's source port"
    );
    cleanup_nodes(&mut [receiver]).await;
}

#[tokio::test]
async fn udp_handshake_reply_never_resolves_an_uncached_hostname() {
    let mut receiver = spanning_tree::make_test_node().await;
    let resolver = Arc::new(CountingResolver(AtomicUsize::new(0)));
    let Some(TransportHandle::Udp(udp)) = receiver.node.transports.remove(&receiver.transport_id)
    else {
        panic!("expected a UDP fixture");
    };
    receiver.node.transports.insert(
        receiver.transport_id,
        TransportHandle::Udp(udp.with_host_network(Some(resolver.clone()))),
    );
    let packet = incoming_msg1(&receiver, TransportAddr::from_string("peer.example:2121"));
    receiver.node.handle_msg1(packet).await;
    assert_eq!(
        receiver.node.peer_count(),
        1,
        "the valid Msg1 must be processed"
    );
    assert_eq!(
        resolver.0.load(Ordering::SeqCst),
        0,
        "reply sends must not invoke DNS"
    );
    cleanup_nodes(&mut [receiver]).await;
}
