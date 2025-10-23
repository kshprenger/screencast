mod errors;

use crate::transport::errors::TransportErrors;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::SinkExt;
use futures_util::StreamExt;
use std::collections::HashMap;
use std::net::Ipv6Addr;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::{self, Utf8Bytes};
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::WebSocketStream;
use uuid::Uuid;
use webrtc::api::{APIBuilder, API};
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc_model::{RoutedSignallingMessage, Routing, SignallingMessage};

type WsTx = SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, tungstenite::protocol::Message>;
type WsRx = SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;

pub struct WebrtcTransport {
    signalling_server_address: Ipv6Addr,
    signalling_server_port: u16,
    self_id: Uuid,
    webrtc_api: API,
    default_config: RTCConfiguration,
    conns_state: Mutex<PeersState>,
}

struct PeersState {
    ws_tx: Option<WsTx>,
    peers: HashMap<Uuid, Option<RTCPeerConnection>>,
}

impl WebrtcTransport {
    fn new(address: Ipv6Addr, port: u16) -> Arc<Self> {
        let ice_servers = vec![RTCIceServer {
            urls: vec![
                "stun:stun.l.google.com:19302".to_string(), // Backuping with 4 different servers
                "stun:stun.l.google.com:5349".to_string(),
                "stun:stun1.l.google.com:3478".to_string(),
                "stun:stun1.l.google.com:5349".to_string(),
            ],
            ..Default::default()
        }];

        let config = RTCConfiguration {
            ice_servers,
            ..Default::default()
        };

        let api = APIBuilder::new().build();

        Arc::new(Self {
            signalling_server_address: address,
            signalling_server_port: port,
            self_id: Uuid::new_v4(),
            webrtc_api: api,
            default_config: config,
            conns_state: Mutex::new(PeersState {
                ws_tx: None,
                peers: HashMap::new(),
            }),
        })
    }

    async fn connect(self: &Arc<Self>) -> Result<WsRx, TransportErrors> {
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
                self.conns_state.lock().await.ws_tx = Some(tx);
                Ok(rx)
            }
            Err(err) => {
                tracing::error!("Could not open ws with signalling server {err}");
                Err(TransportErrors::ConnectionFailed)
            }
        }
    }

    async fn send_signalling_message(
        self: &Arc<Self>,
        message: RoutedSignallingMessage,
    ) -> Result<(), TransportErrors> {
        if let Some(ws_tx) = &mut self.conns_state.lock().await.ws_tx {
            if let Err(err) = ws_tx
                .send(tungstenite::Message::Text(Utf8Bytes::from(
                    serde_json::to_string(&message)?,
                )))
                .await
            {
                tracing::error!("Failed to send text message: {err}");
                return Err(TransportErrors::SendControlFailed);
            }
            return Ok(());
        }
        Err(TransportErrors::ConnectionIsNotOpened)
    }

    async fn handle_signalling_message(self: &Arc<Self>, routed_message: RoutedSignallingMessage) {
        match routed_message.message {
            SignallingMessage::NewPeer { peer_id } => {
                tracing::info!("New peer connected, peer_is: {}", peer_id);
                self.conns_state.lock().await.peers.insert(peer_id, None);

                // Build all to all network topology.
                // If somebody joins network broadcasting itself, we should reply back only to that peer
                // in order not to flood network with messages.
                match routed_message.routing {
                    Routing::Broadcast => {
                        if let Err(err) = self
                            .send_signalling_message(RoutedSignallingMessage {
                                routing: Routing::To(peer_id),
                                message: SignallingMessage::NewPeer {
                                    peer_id: self.self_id,
                                },
                            })
                            .await
                        {
                            tracing::error!("Could not reply to new_peer message: {err}");
                        };
                    }
                    Routing::To(peer_id) => {
                        tracing::debug!("Got direct new_peer message from: {peer_id}")
                    }
                }
            }
            SignallingMessage::PeerLeft { peer_id } => {
                tracing::info!("Peer disconnected, peer_is: {}", peer_id);
                self.conns_state.lock().await.peers.remove(&peer_id);
            }
            SignallingMessage::Offer { peer_id, sdp } => {
                tracing::info!("Received SDP Offer from peer:{peer_id}, offer:{:?}", sdp);
                let conn = match self
                    .webrtc_api
                    .new_peer_connection(self.default_config.clone())
                    .await
                {
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

                let answer = match conn.create_answer(None).await {
                    Ok(answer) => answer,
                    Err(err) => {
                        tracing::error!("Could not create sdp answer: {}", err);
                        return;
                    }
                };

                if let Err(err) = conn.set_local_description(answer.clone()).await {
                    tracing::error!("Failed to set local description: {}", err);
                    return;
                }

                self.conns_state
                    .lock()
                    .await
                    .peers
                    .insert(peer_id, Some(conn));

                if let Err(err) = self
                    .send_signalling_message(RoutedSignallingMessage {
                        routing: Routing::To(peer_id),
                        message: SignallingMessage::Answer {
                            peer_id: self.self_id,
                            sdp: answer,
                        },
                    })
                    .await
                {
                    tracing::error!("Could not answer on offer: {}", err);
                }
            }
            SignallingMessage::Answer { peer_id, sdp } => {
                tracing::info!("Received SDP Answer: {:?}", sdp);
                if let Err(err) = self.conns_state.lock().await.peers[&peer_id]
                    .as_ref()
                    .unwrap() // We receiving answer only after creating offer
                    .set_remote_description(sdp)
                    .await
                {
                    tracing::error!("Failed to set remote description: {:?}", err);
                }
            }
            SignallingMessage::IceCandidate { ice_candidate } => {
                tracing::info!("Received ICE Candidate: {:?}", ice_candidate);
                todo!();
            }
        }
    }
}

impl WebrtcTransport {
    pub async fn join_peer_network(self: &Arc<Self>) -> Result<(), TransportErrors> {
        self.send_signalling_message(RoutedSignallingMessage {
            routing: Routing::Broadcast,
            message: SignallingMessage::NewPeer {
                peer_id: self.self_id,
            },
        })
        .await
    }

    pub async fn offer(
        self: &Arc<Self>,
        sdp: RTCSessionDescription,
    ) -> Result<(), TransportErrors> {
        self.send_signalling_message(RoutedSignallingMessage {
            routing: Routing::Broadcast,
            message: SignallingMessage::Offer {
                peer_id: self.self_id,
                sdp,
            },
        })
        .await
    }

    // Blocks
    pub async fn connect_and_handle(
        self: &Arc<Self>,
        ctx: tokio_util::sync::CancellationToken,
    ) -> Result<(), TransportErrors> {
        let mut rx = self.connect().await?;
        let ctx = ctx.clone();
        tokio::select! {
            _ = ctx.cancelled() => {
                tracing::warn!("Done singalling server handling routine");
            }
            _ = async {
                while let Some(Ok(tungstenite::Message::Text(text))) = rx.next().await {
                    if let Ok(message) = serde_json::from_str::<RoutedSignallingMessage>(&text) {
                        self.handle_signalling_message(message).await;
                    } else {
                        tracing::warn!("Failed to parse incoming message: {}", text);
                    }
                }
            } => {}
        }
        Ok(())
    }
}
