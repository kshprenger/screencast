use serde::{Deserialize, Serialize};
use uuid::Uuid;
use webrtc::{
    ice_transport::ice_candidate::RTCIceCandidate,
    peer_connection::sdp::session_description::RTCSessionDescription,
};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Routing {
    Broadcast,
    To(Uuid),
    From(Box<Routing>, Uuid), // Only server assigns that type
}

pub fn add_from(routing: Routing, from: Uuid) -> Routing {
    Routing::From(Box::new(routing), from)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "kind")]
pub enum SignallingMessage {
    NewPeer,
    PeerLeft,
    Offer { sdp: RTCSessionDescription },
    Answer { sdp: RTCSessionDescription },
    ICECandidate { candidate: RTCIceCandidate },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RoutedSignallingMessage {
    pub routing: Routing,
    pub message: SignallingMessage,
}
