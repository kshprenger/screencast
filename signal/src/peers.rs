use futures_util::future;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::mpsc;
use tokio::sync::RwLock;
use uuid::Uuid;
use webrtc_model::add_from;
use webrtc_model::RoutedSignallingMessage;
use webrtc_model::Routing;
use webrtc_model::SignallingMessage;

type PeersMap = Arc<RwLock<HashMap<Uuid, mpsc::Sender<String>>>>;

#[derive(Clone, Default)]
pub struct PeerManager {
    peers: PeersMap,
}

impl PeerManager {
    pub fn new() -> Self {
        Self {
            peers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn add_peer(&self, id: Uuid, tx: mpsc::Sender<String>) {
        self.peers.write().await.insert(id, tx);
        self.send_message(RoutedSignallingMessage {
            routing: add_from(Routing::Broadcast, id),
            message: webrtc_model::SignallingMessage::NewPeer,
        })
        .await;
        tracing::info!("Peer {id} connected ");
    }

    pub async fn remove_peer(&self, id: Uuid) {
        self.peers.write().await.remove(&id);
        self.send_message(RoutedSignallingMessage {
            routing: add_from(Routing::Broadcast, id),
            message: SignallingMessage::PeerLeft,
        })
        .await;
        tracing::info!("Peer {id} disconnected");
    }

    pub async fn send_message(&self, message: RoutedSignallingMessage) {
        let peer_map = self.peers.read().await;

        match serde_json::to_string(&message) {
            Ok(serialized_message) => match message.routing {
                Routing::From(inner_routing, from) => match *inner_routing {
                    Routing::Broadcast => {
                        future::join_all(peer_map.iter().filter(|(uuid, _)| **uuid != from).map(
                            |(_, peer_sender)| async {
                                let _ = peer_sender.send(serialized_message.clone()).await;
                            },
                        ))
                        .await;
                    }
                    Routing::To(target_uuid) => {
                        if let Some(peer_sender) = peer_map.get(&target_uuid) {
                            let _ = peer_sender.send(serialized_message).await;
                        }
                    }
                    _ => unreachable!(),
                },
                _ => unreachable!(),
            },
            Err(err) => tracing::error!("Could not serialize message: {err}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;
    use tokio::time::{timeout, Duration};
    use webrtc_model::{Routing, SignallingMessage};

    #[tokio::test]
    async fn test_new_peer_manager() {
        let manager = PeerManager::new();
        let peers = manager.peers.read().await;
        assert!(peers.is_empty());
    }

    #[tokio::test]
    async fn test_default_peer_manager() {
        let manager = PeerManager::default();
        let peers = manager.peers.read().await;
        assert!(peers.is_empty());
    }

    #[tokio::test]
    async fn test_add_peer() {
        let manager = PeerManager::new();
        let (tx1, mut rx1) = mpsc::channel::<String>(10);
        let (tx2, mut rx2) = mpsc::channel::<String>(10);
        let peer_id1 = Uuid::new_v4();
        let peer_id2 = Uuid::new_v4();

        manager.add_peer(peer_id1, tx1).await;

        let result = timeout(Duration::from_millis(50), rx1.recv()).await;
        assert!(result.is_err());

        manager.add_peer(peer_id2, tx2).await;

        let peers = manager.peers.read().await;
        assert_eq!(peers.len(), 2);
        assert!(peers.contains_key(&peer_id1));
        assert!(peers.contains_key(&peer_id2));
        drop(peers);

        let message = timeout(Duration::from_millis(100), rx1.recv())
            .await
            .expect("Should receive message within timeout")
            .expect("Should receive a message");

        let parsed_message: RoutedSignallingMessage =
            serde_json::from_str(&message).expect("Should deserialize message");

        match parsed_message.message {
            SignallingMessage::NewPeer => {
                // NewPeer doesn't contain peer_id, it's in the routing
            }
            _ => panic!("Expected NewPeer message"),
        }

        match parsed_message.routing {
            Routing::From(inner_routing, from_id) => {
                assert_eq!(from_id, peer_id2);
                match *inner_routing {
                    Routing::Broadcast => {
                        // This is correct - broadcast from peer_id2
                    }
                    _ => panic!("Expected inner Broadcast routing"),
                }
            }
            _ => panic!("Expected From routing with Broadcast"),
        }

        let result = timeout(Duration::from_millis(50), rx2.recv()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_remove_peer() {
        let manager = PeerManager::new();
        let (tx1, mut rx1) = mpsc::channel::<String>(10);
        let (tx2, mut rx2) = mpsc::channel::<String>(10);
        let peer_id1 = Uuid::new_v4();
        let peer_id2 = Uuid::new_v4();

        manager.add_peer(peer_id1, tx1).await;
        manager.add_peer(peer_id2, tx2).await;

        let _ = rx1.recv().await;

        manager.remove_peer(peer_id1).await;

        let peers = manager.peers.read().await;
        assert_eq!(peers.len(), 1);
        assert!(!peers.contains_key(&peer_id1));
        assert!(peers.contains_key(&peer_id2));
        drop(peers);

        let message = timeout(Duration::from_millis(100), rx2.recv())
            .await
            .expect("Should receive message within timeout")
            .expect("Should receive a message");

        let parsed_message: RoutedSignallingMessage =
            serde_json::from_str(&message).expect("Should deserialize message");

        match parsed_message.message {
            SignallingMessage::PeerLeft => {
                // PeerLeft doesn't contain peer_id, it's in the routing
            }
            _ => panic!("Expected PeerLeft message"),
        }
    }

    #[tokio::test]
    async fn test_remove_nonexistent_peer() {
        let manager = PeerManager::new();
        let peer_id = Uuid::new_v4();

        manager.remove_peer(peer_id).await;

        let peers = manager.peers.read().await;
        assert!(peers.is_empty());
    }

    #[tokio::test]
    async fn test_send_message_broadcast_excluding() {
        let manager = PeerManager::new();
        let (tx1, mut rx1) = mpsc::channel::<String>(10);
        let (tx2, mut rx2) = mpsc::channel::<String>(10);
        let (tx3, mut rx3) = mpsc::channel::<String>(10);
        let peer_id1 = Uuid::new_v4();
        let peer_id2 = Uuid::new_v4();
        let peer_id3 = Uuid::new_v4();

        manager.add_peer(peer_id1, tx1).await;
        manager.add_peer(peer_id2, tx2).await;
        manager.add_peer(peer_id3, tx3).await;

        let _ = rx1.recv().await;
        let _ = rx1.recv().await;
        let _ = rx2.recv().await;

        let test_message = RoutedSignallingMessage {
            routing: Routing::From(Box::new(Routing::Broadcast), peer_id2),
            message: SignallingMessage::NewPeer,
        };

        manager.send_message(test_message).await;

        let msg1 = timeout(Duration::from_millis(100), rx1.recv())
            .await
            .expect("Should receive message within timeout")
            .expect("Should receive a message");

        let msg3 = timeout(Duration::from_millis(100), rx3.recv())
            .await
            .expect("Should receive message within timeout")
            .expect("Should receive a message");

        let parsed1: RoutedSignallingMessage = serde_json::from_str(&msg1).unwrap();
        let parsed3: RoutedSignallingMessage = serde_json::from_str(&msg3).unwrap();

        assert!(
            matches!(parsed1.routing, Routing::From(ref inner, id) if matches!(**inner, Routing::Broadcast) && id == peer_id2)
        );
        assert!(
            matches!(parsed3.routing, Routing::From(ref inner, id) if matches!(**inner, Routing::Broadcast) && id == peer_id2)
        );

        let result = timeout(Duration::from_millis(50), rx2.recv()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_send_message_to_specific_peer() {
        let manager = PeerManager::new();
        let (tx1, mut rx1) = mpsc::channel::<String>(10);
        let (tx2, mut rx2) = mpsc::channel::<String>(10);
        let peer_id1 = Uuid::new_v4();
        let peer_id2 = Uuid::new_v4();

        manager.add_peer(peer_id1, tx1).await;
        manager.add_peer(peer_id2, tx2).await;

        let _ = rx1.recv().await;

        let test_message = RoutedSignallingMessage {
            routing: Routing::From(Box::new(Routing::To(peer_id1)), peer_id2),
            message: SignallingMessage::NewPeer,
        };

        manager.send_message(test_message).await;

        let msg1 = timeout(Duration::from_millis(100), rx1.recv())
            .await
            .expect("Should receive message within timeout")
            .expect("Should receive a message");

        let parsed1: RoutedSignallingMessage = serde_json::from_str(&msg1).unwrap();
        assert!(
            matches!(parsed1.routing, Routing::From(ref inner, from_id) if matches!(**inner, Routing::To(to_id) if to_id == peer_id1) && from_id == peer_id2)
        );

        let result = timeout(Duration::from_millis(50), rx2.recv()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_send_message_to_nonexistent_peer() {
        let manager = PeerManager::new();
        let nonexistent_id = Uuid::new_v4();

        let test_message = RoutedSignallingMessage {
            routing: Routing::From(Box::new(Routing::To(nonexistent_id)), Uuid::new_v4()),
            message: SignallingMessage::NewPeer,
        };

        manager.send_message(test_message).await;
    }

    #[tokio::test]
    async fn test_multiple_peers_lifecycle() {
        let manager = PeerManager::new();
        let mut channels = Vec::new();
        let mut peer_ids = Vec::new();

        for _ in 0..5 {
            let (tx, rx) = mpsc::channel::<String>(10);
            let peer_id = Uuid::new_v4();
            channels.push(rx);
            peer_ids.push(peer_id);
            manager.add_peer(peer_id, tx).await;
        }

        {
            let peers = manager.peers.read().await;
            assert_eq!(peers.len(), 5);
            for peer_id in &peer_ids {
                assert!(peers.contains_key(peer_id));
            }
        }

        for peer_id in peer_ids.iter().take(3) {
            manager.remove_peer(*peer_id).await;
        }

        {
            let peers = manager.peers.read().await;
            assert_eq!(peers.len(), 2);
            for peer_id in peer_ids.iter().take(3) {
                assert!(!peers.contains_key(peer_id));
            }
            for peer_id in peer_ids.iter().skip(3) {
                assert!(peers.contains_key(peer_id));
            }
        }
    }

    #[tokio::test]
    async fn test_clone_peer_manager() {
        let manager1 = PeerManager::new();
        let manager2 = manager1.clone();

        let (tx, _rx) = mpsc::channel::<String>(10);
        let peer_id = Uuid::new_v4();

        manager1.add_peer(peer_id, tx).await;

        {
            let peers1 = manager1.peers.read().await;
            let peers2 = manager2.peers.read().await;
            assert_eq!(peers1.len(), 1);
            assert_eq!(peers2.len(), 1);
            assert!(peers1.contains_key(&peer_id));
            assert!(peers2.contains_key(&peer_id));
        }
    }
}
