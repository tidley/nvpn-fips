//! Non-dialing node sends, adapted from upstream FIPS 76f819451.
use super::*;

impl TorTransport {
    /// Send a packet, establishing a SOCKS5 connection if needed.
    pub async fn send_async(
        &self,
        addr: &TransportAddr,
        data: &[u8],
    ) -> Result<usize, TransportError> {
        self.send_record(addr, data, true).await
    }

    /// Send on a pooled connection only, without waiting for a SOCKS5 dial.
    pub(crate) async fn send_existing(
        &self,
        addr: &TransportAddr,
        data: &[u8],
    ) -> Result<usize, TransportError> {
        let _ = self.connection_state_sync(addr);
        self.send_record(addr, data, false).await
    }

    async fn send_record(
        &self,
        addr: &TransportAddr,
        data: &[u8],
        allow_connect: bool,
    ) -> Result<usize, TransportError> {
        if !self.state.is_operational() {
            return Err(TransportError::NotStarted);
        }
        if let Err(error) = validate_stream_record(data) {
            self.stats.record_send_error();
            return Err(TransportError::SendFailed(format!(
                "invalid FIPS stream record: {error}"
            )));
        }
        let io = {
            let pool = lock_pool(&self.pool);
            pool.get(addr).map(|connection| connection.io.clone())
        };
        let io = match io {
            Some(io) if !io.is_closed() => io,
            _ if !allow_connect => {
                self.stats.record_send_error();
                return Err(TransportError::SendFailed(
                    "no existing Tor connection".into(),
                ));
            }
            _ => self.connect(addr).await?,
        };

        // Public sends keep their existing contract. Node writes retain the
        // cancellation-safe record writer, but cannot wait forever on a peer.
        let deadline =
            (!allow_connect).then(|| tokio::time::Instant::now() + Duration::from_millis(500));
        match io.write_record(data, deadline).await {
            Ok(()) => {
                self.stats.record_send(data.len());
                trace!(
                    transport_id = %self.transport_id,
                    remote_addr = %addr,
                    bytes = data.len(),
                    "Tor packet sent"
                );
                Ok(data.len())
            }
            Err(error) => {
                self.stats.record_send_error();
                if error.poisons_connection() {
                    self.remove_failed_connection(addr, &io).await;
                }
                match error {
                    StreamWriteError::LockTimeout | StreamWriteError::WriteTimeout => {
                        Err(TransportError::Timeout)
                    }
                    StreamWriteError::Closed | StreamWriteError::Io(_) => {
                        Err(TransportError::SendFailed(error.to_string()))
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn missing_tor_connection_never_contacts_the_proxy() {
        let proxy = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (tx, _rx) = crate::transport::packet_channel(16);
        let mut transport = TorTransport::new(
            TransportId::new(1),
            None,
            TorConfig {
                socks5_addr: Some(proxy.local_addr().unwrap().to_string()),
                ..Default::default()
            },
            tx,
        );
        transport.start_async().await.unwrap();
        let address = TransportAddr::from_string("127.0.0.1:2121");
        let mut wire = vec![0; 114];
        wire[0] = 1;
        wire[2..4].copy_from_slice(&110u16.to_le_bytes());
        assert!(transport.send_existing(&address, &wire).await.is_err());
        assert!(
            tokio::time::timeout(Duration::from_millis(50), proxy.accept())
                .await
                .is_err()
        );
        transport.stop_async().await.unwrap();
    }
}
