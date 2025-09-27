use axum::{
    extract::{
        ws::{Message, WebSocket},
        State, WebSocketUpgrade,
    },
    http::Method,
    response::IntoResponse,
    routing::get,
    Router,
};
use futures_util::{sink::SinkExt, stream::StreamExt};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::sync::{broadcast, Mutex};
use tower_http::cors::{Any, CorsLayer};
use tracing_subscriber::FmtSubscriber;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SDP {
    sdp: String,
    #[serde(rename = "type")]
    type_: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct IceCandidate {
    candidate: String,
    sdp_m_line_index: u32,
    sdp_mid: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
enum SignalingMessage {
    Offer(SDP),
    Answer(SDP),
    IceCandidate(IceCandidate),
    NewPeer(Uuid),
    PeerLeft(Uuid),
}

type Peers = Arc<Mutex<HashMap<Uuid, broadcast::Sender<String>>>>;

fn setup_logging() {
    let subscriber = FmtSubscriber::builder()
        .with_max_level(tracing::Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
}

fn setup_cors() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Method::GET)
        .allow_headers(Any)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    setup_logging();
    let cors = setup_cors();

    let peers: Peers = Arc::new(Mutex::new(HashMap::new()));

    let app = Router::new()
        .route("/ws", get(ws_handler))
        .with_state(peers)
        .layer(cors);

    let addr = SocketAddr::from(([0, 0, 0, 0], 35080));
    tracing::info!("Starting server on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service()).await?;
    Ok(())
}

async fn ws_handler(ws: WebSocketUpgrade, State(peers): State<Peers>) -> impl IntoResponse {
    let id = Uuid::new_v4();
    tracing::info!("New peer connecting: {}", id);
    ws.on_upgrade(move |socket| handle_socket(socket, peers, id))
}

async fn handle_socket(socket: WebSocket, peers: Peers, id: Uuid) {
    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = broadcast::channel::<String>(100);

    {
        let mut peer_map = peers.lock().await;

        // Announce the new peer to existing peers
        let new_peer_msg = serde_json::to_string(&SignalingMessage::NewPeer(id)).unwrap();
        for (peer_id, peer_tx) in peer_map.iter() {
            tracing::info!("Notifying peer {} about new peer {}", peer_id, id);
            if let Err(e) = peer_tx.send(new_peer_msg.clone()) {
                tracing::error!(
                    "Failed to send new peer notification to peer {}: {}",
                    peer_id,
                    e
                );
            }
        }
        peer_map.insert(id, tx.clone());
        tracing::info!("Peer {} connected successfully", id);
    }

    // Task to send messages from the broadcast channel to the WebSocket client
    let mut send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            if sender.send(Message::Text(msg)).await.is_err() {
                break;
            }
        }
    });

    let peers_clone = peers.clone();

    // Task to receive messages from the WebSocket client and broadcast them
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(Message::Text(text))) = receiver.next().await {
            let peer_map = peers_clone.lock().await;
            // Find the sender for the current peer
            if let Some(own_tx) = peer_map.get(&id) {
                // Broadcast the message to all peers (including the sender, which is fine)
                if let Err(e) = own_tx.send(text) {
                    tracing::error!("Failed to broadcast message from {}: {}", id, e);
                }
            }
        }
    });

    // Wait for either task to complete and then cancel abort one
    tokio::select! {
        _ = (&mut send_task) => recv_task.abort(),
        _ = (&mut recv_task) => send_task.abort(),
    };

    tracing::info!("Peer {} disconnected", id);

    {
        let mut peer_map = peers.lock().await;
        peer_map.remove(&id);
        let peer_left_msg = serde_json::to_string(&SignalingMessage::PeerLeft(id)).unwrap();
        for (peer_id, peer_tx) in peer_map.iter() {
            if let Err(e) = peer_tx.send(peer_left_msg.clone()) {
                tracing::error!(
                    "Failed to send peer left notification to peer {}: {}",
                    peer_id,
                    e
                );
            }
        }
    }
}
