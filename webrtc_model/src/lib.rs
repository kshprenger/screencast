use serde::{Deserialize, Serialize};
use uuid::Uuid;
use webrtc::{
    ice_transport::ice_candidate::RTCIceCandidate,
    peer_connection::sdp::session_description::RTCSessionDescription,
};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Routing {
    BroadcastExcluding(Uuid),
    To(Uuid),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "kind")]
pub enum SignallingMessage {
    Offer {
        from: Uuid,
        sdp: RTCSessionDescription,
    },
    Answer {
        from: Uuid,
        sdp: RTCSessionDescription,
    },
    NewPeer {
        peer_id: Uuid,
    },
    PeerLeft {
        peer_id: Uuid,
    },
    ICECandidate {
        from: Uuid,
        candidate: RTCIceCandidate,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RoutedSignallingMessage {
    pub routing: Routing,
    pub message: SignallingMessage,
}
