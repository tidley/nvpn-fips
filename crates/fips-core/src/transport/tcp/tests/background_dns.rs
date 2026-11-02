use super::*;
use crate::host_network::HostNetwork;
use std::future::Future;
use std::io;
use std::pin::Pin;

#[derive(Debug)]
struct HangingResolver;

impl HostNetwork for HangingResolver {
    fn protect_socket(&self, _: &socket2::SockRef<'_>) -> io::Result<()> {
        Ok(())
    }

    fn resolve<'a>(
        &'a self,
        _: &'a str,
    ) -> Pin<Box<dyn Future<Output = io::Result<Vec<SocketAddr>>> + Send + Sync + 'a>> {
        Box::pin(std::future::pending())
    }
}

#[tokio::test(start_paused = true)]
async fn background_connect_does_not_wait_for_dns_on_the_caller() {
    let (tx, _rx) = packet_channel(16);
    let mut transport = TcpTransport::new(TransportId::new(1), None, make_outbound_config(), tx);
    transport.host_network = Some(Arc::new(HangingResolver));
    transport.start_async().await.unwrap();
    let address = TransportAddr::from_string("peer.example:443");
    timeout(Duration::from_millis(10), transport.connect_async(&address))
        .await
        .expect("background connection setup must not wait on the resolver")
        .unwrap();
    assert_eq!(
        transport.connection_state_sync(&address),
        ConnectionState::Connecting
    );
    transport.close_connection_async(&address).await;
    assert_eq!(
        transport.connection_state_sync(&address),
        ConnectionState::None
    );
    transport.stop_async().await.unwrap();
}

#[tokio::test]
async fn send_existing_promotes_completed_background_connection_without_redialing() {
    use tokio::io::AsyncReadExt;
    let (tx, _rx) = packet_channel(16);
    let mut transport = TcpTransport::new(TransportId::new(1), None, make_outbound_config(), tx);
    transport.start_async().await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = TransportAddr::from_string(&listener.local_addr().unwrap().to_string());
    transport.connect_async(&address).await.unwrap();
    let (mut stream, _) = timeout(Duration::from_secs(1), listener.accept())
        .await
        .unwrap()
        .unwrap();
    timeout(Duration::from_secs(1), async {
        while !lock_pool(&transport.connecting)
            .get(&address)
            .unwrap()
            .task
            .is_finished()
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let wire = build_msg1_frame(0x37);
    assert_eq!(
        transport.send_existing(&address, &wire).await.unwrap(),
        wire.len()
    );
    let mut received = vec![0; wire.len()];
    timeout(Duration::from_secs(1), stream.read_exact(&mut received))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(received, wire);
    transport.close_connection_async(&address).await;
    assert!(transport.send_existing(&address, &wire).await.is_err());
    assert!(
        timeout(Duration::from_millis(50), listener.accept())
            .await
            .is_err()
    );
    transport.stop_async().await.unwrap();
}
