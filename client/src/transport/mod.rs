mod errors;

use futures_util::stream::{SplitSink, SplitStream};
use futures_util::SinkExt;
use futures_util::StreamExt;
use uuid::Uuid;
use webrtc::api::{APIBuilder, API};
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::RTCPeerConnection;
use std::collections::HashMap;
use std::net::{Ipv6Addr};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::{self, Utf8Bytes};
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::WebSocketStream;
use webrtc_model::SignalingMessage;
use crate::transport::errors::TransportErrors;

type WsTx = SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, tungstenite::protocol::Message>;
type WsRx = SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;

pub struct WebrtcTransport {
    signalling_server_address: Ipv6Addr,
    signalling_server_port: u16,
    ws_tx: Option<WsTx>,
    webrtc_api: API,
    default_config: RTCConfiguration,
    peers: HashMap<Uuid, RTCPeerConnection>,
}

impl WebrtcTransport {
    fn new(address: Ipv6Addr, port: u16) -> Self {
        let ice_servers = vec![
            RTCIceServer {
                urls: vec![
                    "stun:stun.l.google.com:19302".to_string(),
                    "stun:stun.l.google.com:5349".to_string(),
                    "stun:stun1.l.google.com:3478".to_string(),
                    "stun:stun1.l.google.com:5349".to_string(),
                ],
                ..Default::default()
            },
        ];

        let config = RTCConfiguration {
            ice_servers,
            ..Default::default()
        };

        let api = APIBuilder::new().build();

        Self {
            signalling_server_address: address,
            signalling_server_port: port,
            ws_tx: None,
            webrtc_api: api,
            default_config: config,
            peers: HashMap::new()
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
                return Err(TransportErrors::SendControlFailed);
            }
            return Ok(());
        }
        Err(TransportErrors::ConnectionIsNotOpened)
    }

    async fn handle_incoming_message(&mut self, message: SignalingMessage) {
        match message {
            SignalingMessage::Offer { sdp } => {
                tracing::info!("Received SDP Offer: {:?}", sdp);
                let conn = match self.webrtc_api.new_peer_connection(self.default_config.clone()).await{
                    Ok(conn) => conn,
                    Err(err) => {
                        tracing::error!("Failed to construct new webrtc connection: {}", err);
                        return;
                    }
                };

                if let Err(err) = conn.set_remote_description(sdp).await {
                    tracing::error!("Failed to set remote description: {}", err);
                    return;
                }

                let answer =  match conn.create_answer(None).await {
                    Ok(answer) => answer,
                    Err(err) => {
                        tracing::error!("Could not create sdp answer: {}", err);
                        return
                    }
                };

                if let Err(err) = conn.set_local_description(answer.clone()).await {
                    tracing::error!("Failed to set local description: {}", err);
                    return;
                }

                if let Err(err) = self.send_signal_message(SignalingMessage::Answer { sdp: answer }).await{
                    tracing::error!("Could not answer on offer: {}", err);
                }
            }
            SignalingMessage::Answer { sdp } => {
                tracing::info!("Received SDP Answer: {:?}", sdp);
                if let Err(err) = peer_connection.set_remote_description(sdp).await {
                    tracing::error!("Failed to set remote description: {:?}", err);
                }
            }
            SignalingMessage::IceCandidate { ice_candidate } => {
                tracing::info!("Received ICE Candidate: {:?}", ice_candidate);
                todo!();
            }
            _ => tracing::warn!("Unhandled signaling message: {:?}", message),
        }
    }
}


impl WebrtcTransport {
    pub async fn send_signal_message(&mut self, message: SignalingMessage)  -> Result<(), TransportErrors>{
        self.send_message(serde_json::to_string(&message)?).await
    }

    pub async fn connect_and_handle(&mut self, ctx: tokio_util::sync::CancellationToken) -> Result<(), TransportErrors> {
        let mut rx = self.connect().await?;
        let ctx = ctx.clone();
        tokio::select! {
            _ = ctx.cancelled() => {
                tracing::warn!("Done singalling server handling routine");
            }
            _ = async {
                while let Some(Ok(tungstenite::Message::Text(text))) = rx.next().await {
                    if let Ok(message) = serde_json::from_str::<SignalingMessage>(&text) {
                        self.handle_incoming_message(message).await;
                    } else {
                        tracing::warn!("Failed to parse incoming message: {}", text);
                    }
                }
            } => {}
        }
        Ok(())
    }
}
