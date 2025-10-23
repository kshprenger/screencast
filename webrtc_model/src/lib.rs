use serde::{Deserialize, Serialize};
use uuid::Uuid;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Routing {
    Broadcast,
    To(Uuid),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "kind")]
pub enum SignallingMessage {
    Offer {
        peer_id: Uuid,
        sdp: RTCSessionDescription,
    },
    Answer {
        peer_id: Uuid,
        sdp: RTCSessionDescription,
    },
    NewPeer {
        peer_id: Uuid,
    },
    PeerLeft {
        peer_id: Uuid,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RoutedSignallingMessage {
    pub routing: Routing,
    pub message: SignallingMessage,
}
