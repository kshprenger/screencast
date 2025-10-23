use std::{collections::HashMap, sync::Arc};
use tokio::sync::mpsc;
use tokio::sync::RwLock;
use uuid::Uuid;
use webrtc_model::RoutedSignallingMessage;
use webrtc_model::Routing;

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
            routing: Routing::Broadcast,
            message: webrtc_model::SignallingMessage::NewPeer { peer_id: id },
        })
        .await;
        tracing::info!("Peer {id} connected ");
    }

    pub async fn remove_peer(&self, id: &Uuid) {
        self.peers.write().await.remove(id);
        self.send_message(RoutedSignallingMessage {
            routing: Routing::Broadcast,
            message: webrtc_model::SignallingMessage::PeerLeft { peer_id: *id },
        })
        .await;
        tracing::info!("Peer {id} disconnected");
    }

    pub async fn send_message(&self, message: RoutedSignallingMessage) {
        let peer_map = self.peers.read().await;

        match serde_json::to_string(&message) {
            Ok(serialized_message) => match message.routing {
                Routing::Broadcast => {
                    futures::future::join_all(peer_map.iter().map(|(_, peer_sender)| async {
                        peer_sender.send(serialized_message.clone()).await;
                    }))
                    .await;
                }
                Routing::To(target_uuid) => {
                    futures::future::join_all(
                        peer_map
                            .iter()
                            .filter(|(uuid, _)| **uuid == target_uuid)
                            .map(|(_, peer_sender)| async {
                                peer_sender.send(serialized_message.clone()).await
                            }),
                    )
                    .await;
                }
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
        let (tx, mut rx) = mpsc::channel::<String>(10);
        let peer_id = Uuid::new_v4();

        manager.add_peer(peer_id, tx).await;

        let peers = manager.peers.read().await;
        assert_eq!(peers.len(), 1);
        assert!(peers.contains_key(&peer_id));

        let message = timeout(Duration::from_millis(100), rx.recv())
            .await
            .expect("Should receive message within timeout")
            .expect("Should receive a message");

        let parsed_message: RoutedSignallingMessage =
            serde_json::from_str(&message).expect("Should deserialize message");

        match parsed_message.message {
            SignallingMessage::NewPeer {
                peer_id: received_id,
            } => {
                assert_eq!(received_id, peer_id);
            }
            _ => panic!("Expected NewPeer message"),
        }

        match parsed_message.routing {
            Routing::Broadcast => {}
            _ => panic!("Expected Broadcast routing option"),
        }
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
        let _ = rx1.recv().await;
        let _ = rx2.recv().await;

        manager.remove_peer(&peer_id1).await;

        let peers = manager.peers.read().await;
        assert_eq!(peers.len(), 1);
        assert!(!peers.contains_key(&peer_id1));
        assert!(peers.contains_key(&peer_id2));

        let message = timeout(Duration::from_millis(100), rx2.recv())
            .await
            .expect("Should receive message within timeout")
            .expect("Should receive a message");

        let parsed_message: RoutedSignallingMessage =
            serde_json::from_str(&message).expect("Should deserialize message");

        match parsed_message.message {
            SignallingMessage::PeerLeft {
                peer_id: received_id,
            } => {
                assert_eq!(received_id, peer_id1);
            }
            _ => panic!("Expected PeerLeft message"),
        }
    }

    #[tokio::test]
    async fn test_remove_nonexistent_peer() {
        let manager = PeerManager::new();
        let peer_id = Uuid::new_v4();

        manager.remove_peer(&peer_id).await;

        let peers = manager.peers.read().await;
        assert!(peers.is_empty());
    }

    #[tokio::test]
    async fn test_send_message_to_all() {
        let manager = PeerManager::new();
        let (tx1, mut rx1) = mpsc::channel::<String>(10);
        let (tx2, mut rx2) = mpsc::channel::<String>(10);
        let peer_id1 = Uuid::new_v4();
        let peer_id2 = Uuid::new_v4();

        manager.add_peer(peer_id1, tx1).await;
        manager.add_peer(peer_id2, tx2).await;

        let _ = rx1.recv().await;
        let _ = rx1.recv().await;
        let _ = rx2.recv().await;

        let test_message = RoutedSignallingMessage {
            routing: Routing::Broadcast,
            message: SignallingMessage::NewPeer {
                peer_id: Uuid::new_v4(),
            },
        };

        manager.send_message(test_message.clone()).await;

        let msg1 = timeout(Duration::from_millis(100), rx1.recv())
            .await
            .expect("Should receive message within timeout")
            .expect("Should receive a message");

        let msg2 = timeout(Duration::from_millis(100), rx2.recv())
            .await
            .expect("Should receive message within timeout")
            .expect("Should receive a message");

        let parsed1: RoutedSignallingMessage = serde_json::from_str(&msg1).unwrap();
        let parsed2: RoutedSignallingMessage = serde_json::from_str(&msg2).unwrap();

        assert!(matches!(parsed1.routing, Routing::Broadcast));
        assert!(matches!(parsed2.routing, Routing::Broadcast));
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
        let _ = rx1.recv().await;
        let _ = rx2.recv().await;

        let test_message = RoutedSignallingMessage {
            routing: Routing::To(peer_id1),
            message: SignallingMessage::NewPeer {
                peer_id: Uuid::new_v4(),
            },
        };

        manager.send_message(test_message).await;

        let msg1 = timeout(Duration::from_millis(100), rx1.recv())
            .await
            .expect("Should receive message within timeout")
            .expect("Should receive a message");

        let parsed1: RoutedSignallingMessage = serde_json::from_str(&msg1).unwrap();
        assert!(matches!(parsed1.routing, Routing::To(id) if id == peer_id1));

        let result = timeout(Duration::from_millis(50), rx2.recv()).await;
        assert!(result.is_err(), "Peer2 should not receive any message");
    }

    #[tokio::test]
    async fn test_send_message_to_nonexistent_peer() {
        let manager = PeerManager::new();
        let nonexistent_id = Uuid::new_v4();

        let test_message = RoutedSignallingMessage {
            routing: Routing::To(nonexistent_id),
            message: SignallingMessage::NewPeer {
                peer_id: Uuid::new_v4(),
            },
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
            manager.remove_peer(peer_id).await;
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
