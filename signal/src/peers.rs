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
            serde_json::to_string(&webrtc_model::SignalingMessage::NewPeer { peer_id: id })
                .unwrap();
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
            serde_json::to_string(&webrtc_model::SignalingMessage::PeerLeft { peer_id: *id })
                .unwrap();
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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_add_peer() {
        let manager = PeerManager::new();
        let peer_id = Uuid::new_v4();
        let (tx, _rx) = mpsc::channel(1);

        manager.add_peer(peer_id, tx).await;

        let peers = manager.peers.lock().await;
        assert_eq!(peers.len(), 1);
        assert!(peers.contains_key(&peer_id));
    }

    #[tokio::test]
    async fn test_remove_peer() {
        let manager = PeerManager::new();
        let peer_id = Uuid::new_v4();
        let (tx, _rx) = mpsc::channel(1);

        manager.add_peer(peer_id, tx).await;
        assert_eq!(manager.peers.lock().await.len(), 1);

        manager.remove_peer(&peer_id).await;
        assert!(manager.peers.lock().await.is_empty());
    }

    #[tokio::test]
    async fn test_add_peer_notifies_others() {
        let manager = PeerManager::new();

        let peer1_id = Uuid::new_v4();
        let (tx1, mut rx1) = mpsc::channel(10);
        manager.add_peer(peer1_id, tx1).await;

        let peer2_id = Uuid::new_v4();
        let (tx2, _rx2) = mpsc::channel(10);
        manager.add_peer(peer2_id, tx2).await;

        // Check if peer1 received notification about peer2
        let received_msg = rx1.recv().await.unwrap();
        let expected_msg =
            serde_json::to_string(&webrtc_model::SignalingMessage::NewPeer { peer_id: peer2_id })
                .unwrap();
        assert_eq!(received_msg, expected_msg);
    }

    #[tokio::test]
    async fn test_remove_peer_notifies_others() {
        let manager = PeerManager::new();

        let peer1_id = Uuid::new_v4();
        let (tx1, mut rx1) = mpsc::channel(10);
        manager.add_peer(peer1_id, tx1).await;

        let peer2_id = Uuid::new_v4();
        let (tx2, _rx2) = mpsc::channel(10);
        manager.add_peer(peer2_id, tx2).await;

        // Clear the notification from adding peer2
        let _ = rx1.recv().await;

        manager.remove_peer(&peer2_id).await;

        // Check if peer1 received notification about peer2 leaving
        let received_msg = rx1.recv().await.unwrap();
        let expected_msg =
            serde_json::to_string(&webrtc_model::SignalingMessage::PeerLeft { peer_id: peer2_id })
                .unwrap();
        assert_eq!(received_msg, expected_msg);
    }

    #[tokio::test]
    async fn test_broadcast_message() {
        let manager = PeerManager::new();
        let peer_id = Uuid::new_v4();
        let (tx, mut rx) = mpsc::channel(1);

        manager.add_peer(peer_id, tx).await;

        let message = "hello".to_string();
        manager.broadcast_message(peer_id, message.clone()).await;

        let received = rx.recv().await.unwrap();
        assert_eq!(received, message);
    }
}
