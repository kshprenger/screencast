mod connection;
mod errors;
mod events;
mod state;

pub use events::WebrtcEvents;

use crate::network::errors::NetworkErrors;
use crate::network::state::State;
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::SinkExt;
use futures_util::StreamExt;
use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex, MutexGuard};
use tokio::time::sleep;
use tokio_tungstenite::tungstenite::{self, Utf8Bytes};
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::WebSocketStream;
use uuid::Uuid;
use webrtc::api::media_engine::{MediaEngine, MIME_TYPE_H264};
use webrtc::api::{APIBuilder, API};
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::media::Sample;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::rtp_transceiver::rtp_codec::RTCRtpCodecCapability;
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;
use webrtc_model::{RoutedSignallingMessage, Routing, SignallingMessage};

type WsTx = SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, tungstenite::protocol::Message>;
type WsRx = SplitStream<WebSocketStream<MaybeTlsStream<TcpStream>>>;

pub struct WebrtcNetwork {
    signalling_server_address: Ipv4Addr,
    signalling_server_port: u16,
    webrtc_api: API,
    default_config: RTCConfiguration,
    conns_state: Mutex<PeersState>,
}

struct PeersState {
    state: State,
    events_tx: Option<mpsc::UnboundedSender<WebrtcEvents>>, // Webrtc --events--> GUI
    ws_tx: Option<WsTx>,
    track: Option<Arc<TrackLocalStaticSample>>,
    peers: HashMap<Uuid, RTCPeerConnection>,
}

impl WebrtcNetwork {
    async fn connect(self: &Arc<Self>) -> Result<WsRx, NetworkErrors> {
        let url = format!(
            "ws://{}:{}/ws",
            self.signalling_server_address, self.signalling_server_port
        );
        tracing::info!("Signalling server url is: {url}\nConnecting...");
        match tokio_tungstenite::connect_async(url).await {
            Ok((ws_stream, _)) => {
                tracing::info!("Connection successful!");
                let (tx, rx) = ws_stream.split();
                self.conns_state.lock().await.ws_tx = Some(tx);
                Ok(rx)
            }
            Err(err) => {
                tracing::error!("Could not open ws with signalling server {err}");
                Err(NetworkErrors::ConnectionFailed)
            }
        }
    }

    async fn send_signalling_message(
        peer_state: &mut MutexGuard<'_, PeersState>,
        message: RoutedSignallingMessage,
    ) -> Result<(), NetworkErrors> {
        if let Some(ws_tx) = &mut peer_state.ws_tx {
            if let Err(err) = ws_tx
                .send(tungstenite::Message::Text(Utf8Bytes::from(
                    serde_json::to_string(&message)?,
                )))
                .await
            {
                tracing::error!("Failed to send text message: {err}");
                return Err(NetworkErrors::SendControlFailed);
            }
            return Ok(());
        }
        Err(NetworkErrors::ConnectionIsNotOpened)
    }

    async fn handle_signalling_message(self: &Arc<Self>, routed_message: RoutedSignallingMessage) {
        let (inner_routing, from) = match routed_message.routing {
            Routing::From(inner_routing, from) => (*inner_routing, from),
            _ => unreachable!(),
        };

        match routed_message.message {
            SignallingMessage::NewPeer => {
                tracing::info!("New peer connected, peer_id: {from}");

                let conn = self.create_connection(from).await;

                let mut conns_state = self.conns_state.lock().await;
                conns_state.peers.insert(from, conn);

                // Build all to all network topology.
                // If somebody joins network broadcasting itself, we should reply back only to that peer
                // in order not to flood network with messages.
                match inner_routing {
                    Routing::Broadcast => {
                        if let Err(err) = Self::send_signalling_message(
                            &mut conns_state,
                            RoutedSignallingMessage {
                                routing: Routing::To(from),
                                message: SignallingMessage::NewPeer,
                            },
                        )
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
                self.conns_state.lock().await.peers.remove(&from);
            }
            SignallingMessage::Offer { sdp } => {
                tracing::info!("Received SDP Offer from peer:{from}");

                let mut peer_state = self.conns_state.lock().await;

                let conn = match peer_state.peers.get(&from) {
                    Some(conn) => conn,
                    None => {
                        tracing::warn!(
                            "Could find peer that offers. Possibly it have disconnected before"
                        );
                        return;
                    }
                };

                if let Err(err) = conn.set_remote_description(sdp).await {
                    tracing::error!("Failed to set remote description: {err}");
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

                if let Err(err) = Self::send_signalling_message(
                    &mut peer_state,
                    RoutedSignallingMessage {
                        routing: Routing::To(from),
                        message: SignallingMessage::Answer { sdp: answer },
                    },
                )
                .await
                {
                    tracing::error!("Could not answer on offer: {}", err);
                }
            }
            SignallingMessage::Answer { sdp } => {
                tracing::info!("Received SDP Answer from: {from}");
                let mut state = self.conns_state.lock().await;

                match state.state {
                    State::Idle => {
                        tracing::warn!("Currently not gathering answers so message ignored")
                    }
                    State::GatheringAnswers(remain) => match remain {
                        1 => {
                            state
                                .events_tx
                                .clone()
                                .and_then(|chan| {
                                    if let Err(err) = chan.send(WebrtcEvents::GatheredAnswers) {
                                        tracing::error!(
                                            "Could not send GatheredAnswers event: {err}"
                                        );
                                    }
                                    state.state = State::Idle;
                                    Some(())
                                })
                                .or_else(|| {
                                    tracing::warn!("No GUI event subscription");
                                    Some(())
                                });
                        }
                        _ => state.state = State::GatheringAnswers(remain - 1),
                    },
                }

                let conn = match state.peers.get(&from) {
                    Some(conn) => conn,
                    None => {
                        tracing::warn!(
                            "Could find peer that offers. Possibly it disconnected before"
                        );
                        return;
                    }
                };

                if let Err(err) = conn.set_remote_description(sdp).await {
                    tracing::error!("Failed to set remote description: {:?}", err);
                }
            }
            SignallingMessage::ICECandidate { candidate } => {
                tracing::info!("Received candidate {candidate} from {from}");
                match self.conns_state.lock().await.peers.get(&from) {
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

impl WebrtcNetwork {
    pub fn new_shared(address: Ipv4Addr, port: u16) -> Arc<Self> {
        let config = RTCConfiguration {
            ice_servers: vec![RTCIceServer {
                urls: vec!["stun:stun.l.google.com:19302".to_string()],
                ..Default::default()
            }],
            ..Default::default()
        };

        let mut m = MediaEngine::default();
        m.register_default_codecs().unwrap(); // Safe

        let api = APIBuilder::new().with_media_engine(m).build();

        Arc::new(Self {
            signalling_server_address: address,
            signalling_server_port: port,
            webrtc_api: api,
            default_config: config,
            conns_state: Mutex::new(PeersState {
                ws_tx: None,
                peers: HashMap::new(),
                track: None,
                events_tx: None,
                state: State::Idle,
            }),
        })
    }

    pub async fn create_and_send_offers(self: Arc<Self>) {
        self.create_and_add_track().await;
        let mut peers_state = self.conns_state.lock().await;

        peers_state.state = State::GatheringAnswers(peers_state.peers.len());

        // Avoid borrowing problems
        let peer_ids: Vec<Uuid> = peers_state.peers.keys().cloned().collect();

        for peer_id in peer_ids {
            let conn = peers_state.peers.get(&peer_id).unwrap();
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

            if let Err(err) = Self::send_signalling_message(
                &mut peers_state,
                RoutedSignallingMessage {
                    routing: Routing::To(peer_id),
                    message: SignallingMessage::Offer { sdp: offer },
                },
            )
            .await
            {
                tracing::error!("Failed to send offer to peer {peer_id}: {err}");
            }
        }
    }

    pub async fn create_and_add_track(self: &Arc<Self>) {
        let track = Arc::new(TrackLocalStaticSample::new(
            RTCRtpCodecCapability {
                mime_type: MIME_TYPE_H264.to_owned(),
                ..Default::default()
            },
            "video".to_owned(),
            "webrtc-rs".to_owned(),
        ));

        let mut conn_state = self.conns_state.lock().await;

        futures_util::future::join_all(conn_state.peers.values().map(|conn| async {
            let track_clone = Arc::clone(&track);
            conn.add_track(track_clone).await.unwrap();
        }))
        .await;

        conn_state.track = Some(track);
    }

    pub async fn subscribe(self: &Arc<Self>) -> mpsc::UnboundedReceiver<WebrtcEvents> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.conns_state.lock().await.events_tx = Some(tx);
        rx
    }

    pub async fn send_sample(self: &Arc<Self>, sample: &Sample) {
        match self.conns_state.lock().await.track.as_ref() {
            Some(track) => {
                if let Err(err) = track.write_sample(sample).await {
                    tracing::error!("Could not write sample: {err}")
                }
            }
            None => tracing::warn!("Could not find track to write sample"),
        }
    }

    // Main loop (blocking)
    pub async fn join(self: &Arc<Self>) {
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
