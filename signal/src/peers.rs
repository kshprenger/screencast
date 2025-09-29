use crate::webrtc_model;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{mpsc, Mutex};
use uuid::Uuid;

type PeersMap = Arc<Mutex<HashMap<Uuid, mpsc::Sender<String>>>>;

#[derive(Clone, Default)]
pub struct PeerManager {
    peers: PeersMap,
}

impl PeerManager {
    pub fn new() -> Self {
        Self {
            peers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn add_peer(&self, id: Uuid, tx: mpsc::Sender<String>) {
        let mut peer_map = self.peers.lock().await;

        let new_peer_msg =
            serde_json::to_string(&webrtc_model::SignalingMessage::NewPeer(id)).unwrap();
        for (peer_id, peer_tx) in peer_map.iter() {
            tracing::info!("Notifying peer {} about new peer {}", peer_id, id);
            if let Err(e) = peer_tx.send(new_peer_msg.clone()).await {
                tracing::error!(
                    "Failed to send new peer notification to peer {}: {}",
                    peer_id,
                    e
                );
            }
        }
        peer_map.insert(id, tx);
        tracing::info!("Peer {} connected successfully", id);
    }

    pub async fn remove_peer(&self, id: &Uuid) {
        let mut peer_map = self.peers.lock().await;
        peer_map.remove(id);

        let peer_left_msg =
            serde_json::to_string(&webrtc_model::SignalingMessage::PeerLeft(*id)).unwrap();
        for (peer_id, peer_tx) in peer_map.iter() {
            if let Err(e) = peer_tx.send(peer_left_msg.clone()).await {
                tracing::error!(
                    "Failed to send peer left notification to peer {}: {}",
                    peer_id,
                    e
                );
            }
        }
        tracing::info!("Peer {} disconnected", id);
    }

    pub async fn broadcast_message(&self, sender_id: Uuid, msg: String) {
        let peer_map = self.peers.lock().await;
        if let Some(own_tx) = peer_map.get(&sender_id) {
            if let Err(e) = own_tx.send(msg).await {
                tracing::error!("Failed to broadcast message from {}: {}", sender_id, e);
            }
        }
    }
}
