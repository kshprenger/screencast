mod errors;

use futures_util::stream::{SplitSink, SplitStream};
use futures_util::SinkExt;
use futures_util::StreamExt;
use std::net::Ipv4Addr;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::{self, Utf8Bytes};
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::WebSocketStream;

use crate::transport::errors::TransportErrors;

type WsTx = SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, tungstenite::protocol::Message>;
type WsRx = SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;

pub struct WebrtcTransport {
    signalling_server_address: Ipv4Addr,
    signalling_server_port: u16,
    ws_tx: Option<WsTx>,
}

impl WebrtcTransport {
    fn new(address: Ipv4Addr, port: u16) -> Self {
        Self {
            signalling_server_address: address,
            signalling_server_port: port,
            ws_tx: None,
        }
    }

    async fn connect(&mut self) -> Result<WsRx, TransportErrors> {
        let url = format!(
            "ws://{}:{}/ws",
            self.signalling_server_address, self.signalling_server_port
        );
        tracing::info!("Signalling server url to connects is: {}", url);
        tracing::info!("Connecting...");
        match tokio_tungstenite::connect_async(url).await {
            Ok((ws_stream, _)) => {
                tracing::info!("Connection successful!");
                let (tx, rx) = ws_stream.split();
                self.ws_tx = Some(tx);
                Ok(rx)
            }
            Err(err) => {
                tracing::error!("Could not open ws with signalling server {err}");
                Err(TransportErrors::ConnectionFailed)
            }
        }
    }

    async fn send_message(&mut self, message: impl Into<String>) -> Result<(), TransportErrors> {
        if let Some(ws_tx) = &mut self.ws_tx {
            if let Err(err) = ws_tx
                .send(tungstenite::Message::Text(Utf8Bytes::from(message.into())))
                .await
            {
                tracing::error!("Failed to send text message: {err}");
                return Err(TransportErrors::SendFailed);
            }
            return Ok(());
        }
        Err(TransportErrors::ConnectionIsNotOpened)
    }
}
