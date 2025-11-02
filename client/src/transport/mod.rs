mod errors;

use crate::transport::errors::TransportErrors;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::SinkExt;
use futures_util::StreamExt;
use std::collections::HashMap;
use std::net::Ipv6Addr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::RwLock;
use tokio::time::sleep;
use tokio_tungstenite::tungstenite::{self, Utf8Bytes};
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::WebSocketStream;
use uuid::Uuid;
use webrtc::api::{APIBuilder, API};
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc_model::{RoutedSignallingMessage, Routing, SignallingMessage};

type WsTx = SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, tungstenite::protocol::Message>;
type WsRx = SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;

pub struct WebrtcTransport {
    signalling_server_address: Ipv6Addr,
    signalling_server_port: u16,
    webrtc_api: API,
    default_config: RTCConfiguration,
    conns_state: RwLock<PeersState>,
}

struct PeersState {
    ws_tx: Option<WsTx>,
    peers: HashMap<Uuid, RTCPeerConnection>,
}

impl WebrtcTransport {
    async fn connect(self: &Arc<Self>) -> Result<WsRx, TransportErrors> {
        let url = format!(
            "ws://[{}]:{}/ws",
            self.signalling_server_address, self.signalling_server_port
        );
        tracing::info!("Signalling server url is: {url}");
        tracing::info!("Connecting...");
        match tokio_tungstenite::connect_async(url).await {
            Ok((ws_stream, _)) => {
                tracing::info!("Connection successful!");
                let (tx, rx) = ws_stream.split();
                self.conns_state.write().await.ws_tx = Some(tx);
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
        if let Some(ws_tx) = &mut self.conns_state.write().await.ws_tx {
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

    async fn setup_background_ice_candidates_transmitting(
        self: &Arc<Self>,
        conn: &RTCPeerConnection,
        to: Uuid,
    ) {
        let self_clone1 = Arc::clone(self);
        conn.on_ice_candidate(Box::new(move |candidate| {
            let self_clone2 = Arc::clone(&self_clone1);
            Box::pin(async move {
                if let Some(candidate) = candidate {
                    tracing::info!("Found ICE candidate: {candidate}");
                    if let Err(err) = self_clone2
                        .send_signalling_message(RoutedSignallingMessage {
                            routing: Routing::To(to),
                            message: SignallingMessage::ICECandidate { candidate },
                        })
                        .await
                    {
                        tracing::error!("Could not send ICE candidate to peer: {to}, error: {err}");
                    }
                } else {
                    tracing::info!("ICE gathering completed");
                }
            })
        }));
    }

    async fn handle_signalling_message(self: &Arc<Self>, routed_message: RoutedSignallingMessage) {
        let (inner_routing, from) = match routed_message.routing {
            Routing::From(inner_routing, from) => (*inner_routing, from),
            _ => unreachable!(),
        };

        match routed_message.message {
            SignallingMessage::NewPeer => {
                tracing::info!("New peer connected, peer_id: {from}");

                let conn = match self
                    .webrtc_api
                    .new_peer_connection(self.default_config.clone())
                    .await
                {
                    Ok(conn) => conn,
                    Err(err) => {
                        tracing::error!("Failed to construct new webrtc connection: {err}");
                        return;
                    }
                };

                // What if peer does not know about us yet??
                // Then will it lose our Ice candidate?????
                // NewPeerAck????
                self.setup_background_ice_candidates_transmitting(&conn, from)
                    .await;

                self.conns_state.write().await.peers.insert(from, conn);

                // Build all to all network topology.
                // If somebody joins network broadcasting itself, we should reply back only to that peer
                // in order not to flood network with messages.
                match inner_routing {
                    Routing::Broadcast => {
                        if let Err(err) = self
                            .send_signalling_message(RoutedSignallingMessage {
                                routing: Routing::To(from),
                                message: SignallingMessage::NewPeer,
                            })
                            .await
                        {
                            tracing::error!("Could not reply to new_peer message: {err}");
                        };
                    }
                    Routing::To(_) => {
                        tracing::debug!("Got direct new_peer message from: {from}")
                    }
                    _ => unreachable!(),
                }
            }
            SignallingMessage::PeerLeft => {
                tracing::warn!("Peer {from} disconnected");
                self.conns_state.write().await.peers.remove(&from);
            }
            SignallingMessage::Offer { sdp } => {
                tracing::info!("Received SDP Offer from peer:{from}, offer:{:?}", sdp);

                let state = self.conns_state.read().await;

                let conn = match state.peers.get(&from) {
                    Some(conn) => conn,
                    None => {
                        tracing::warn!(
                            "Could find peer that offers. Possibly it have disconnected before"
                        );
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

                if let Err(err) = self
                    .send_signalling_message(RoutedSignallingMessage {
                        routing: Routing::To(from),
                        message: SignallingMessage::Answer { sdp: answer },
                    })
                    .await
                {
                    tracing::error!("Could not answer on offer: {}", err);
                }
            }
            SignallingMessage::Answer { sdp } => {
                tracing::info!("Received SDP Answer from: {from}, answer: {:?}", sdp);

                let state = self.conns_state.read().await;

                let conn = match state.peers.get(&from) {
                    Some(conn) => conn,
                    None => {
                        tracing::warn!(
                            "Could find peer that offers. Possibly it have disconnected before"
                        );
                        return;
                    }
                };

                if let Err(err) = conn.set_remote_description(sdp).await {
                    tracing::error!("Failed to set remote description: {:?}", err);
                    return;
                }
            }
            SignallingMessage::ICECandidate { candidate } => {
                match self.conns_state.read().await.peers.get(&from) {
                    Some(peer) => {
                        let _ = peer.add_ice_candidate(candidate.to_json().unwrap()).await;
                    }
                    None => tracing::warn!(
                        "Could not add Ice candidate because there is no connection with {from}"
                    ),
                }
            }
        }
    }
}

impl WebrtcTransport {
    pub fn new_shared(address: Ipv6Addr, port: u16) -> Arc<Self> {
        let ice_servers = vec![RTCIceServer {
            urls: vec!["stun:stun.l.google.com:19302".to_string()],
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
            webrtc_api: api,
            default_config: config,
            conns_state: RwLock::new(PeersState {
                ws_tx: None,
                peers: HashMap::new(),
            }),
        })
    }

    pub async fn create_and_send_offers(self: Arc<Self>) {
        let peers_state = self.conns_state.read().await;

        for (&peer_id, conn) in &peers_state.peers {
            let offer = match conn.create_offer(None).await {
                Ok(offer) => offer,
                Err(err) => {
                    tracing::error!("Failed to create offer for peer {peer_id}: {err}");
                    continue;
                }
            };

            if let Err(err) = conn.set_local_description(offer.clone()).await {
                tracing::error!("Failed to set local description for peer {peer_id}: {err}");
                continue;
            }

            if let Err(err) = self
                .send_signalling_message(RoutedSignallingMessage {
                    routing: Routing::To(peer_id),
                    message: SignallingMessage::Offer { sdp: offer },
                })
                .await
            {
                tracing::error!("Failed to send offer to peer {peer_id}: {err}");
            }
        }
    }

    // Main loop (blocking)
    pub async fn join_peer_network(self: &Arc<Self>) {
        let mut retry_delay = Duration::from_millis(100);
        let max_delay = Duration::from_secs(2);
        let backoff_multiplier = 2.0;

        loop {
            match self.connect().await {
                Ok(mut rx) => {
                    retry_delay = Duration::from_millis(100);

                    while let Some(msg_result) = rx.next().await {
                        match msg_result {
                            Ok(tungstenite::Message::Text(text)) => {
                                if let Ok(message) =
                                    serde_json::from_str::<RoutedSignallingMessage>(&text)
                                {
                                    self.handle_signalling_message(message).await;
                                } else {
                                    tracing::error!("Failed to parse incoming message: {text}");
                                }
                            }
                            Ok(tungstenite::Message::Close(_)) => {
                                tracing::warn!("WebSocket connection closed by server");
                                break;
                            }
                            Err(e) => {
                                tracing::error!("WebSocket error: {e}");
                                break;
                            }
                            _ => {} // Ignore other message types
                        }
                    }
                    tracing::info!("Connection lost, retrying in {:?}", retry_delay);
                }
                Err(e) => {
                    tracing::error!(
                        "Failed to connect to signalling server: {e}, retrying in {:?}",
                        retry_delay
                    );
                }
            }

            sleep(retry_delay).await;

            retry_delay = std::cmp::min(
                Duration::from_millis((retry_delay.as_millis() as f64 * backoff_multiplier) as u64),
                max_delay,
            );
        }
    }
}
