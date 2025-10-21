use std::future::Future;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::mpsc;
use tokio::sync::RwLock;
use uuid::Uuid;
use webrtc_model::RoutedSignallingMessage;
use webrtc_model::RoutingOptions;

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
            route: RoutingOptions::All,
            message: webrtc_model::SignallingMessage::NewPeer { peer_id: id },
        })
        .await;
        tracing::info!("Peer {id} connected ");
    }

    pub async fn remove_peer(&self, id: &Uuid) {
        self.peers.write().await.remove(id);
        self.send_message(RoutedSignallingMessage {
            route: RoutingOptions::All,
            message: webrtc_model::SignallingMessage::PeerLeft { peer_id: *id },
        })
        .await;
        tracing::info!("Peer {id} disconnected");
    }

    pub async fn send_message(&self, message: RoutedSignallingMessage) {
        let peer_map = self.peers.read().await;

        match serde_json::to_string(&message) {
            Ok(serialized_message) => match message.route {
                RoutingOptions::All => {
                    futures::future::join_all(peer_map.iter().map(|(_, peer_sender)| async {
                        peer_sender.send(serialized_message.clone()).await;
                    }))
                    .await;
                }
                RoutingOptions::To(target_uuid) => {
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
