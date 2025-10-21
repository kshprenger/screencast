use serde::{Deserialize, Serialize};
use uuid::Uuid;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct IceCandidate {
    candidate: String,
    sdp_m_line_index: u32,
    sdp_mid: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum RoutingOptions {
    All,
    To(Uuid),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum SignallingMessage {
    Offer {
        peer_id: Uuid,
        sdp: RTCSessionDescription,
    },
    Answer {
        peer_id: Uuid,
        sdp: RTCSessionDescription,
    },
    IceCandidate {
        ice_candidate: IceCandidate,
    },
    NewPeer {
        routing: RoutingOptions,
        peer_id: Uuid,
    },
    PeerLeft {
        peer_id: Uuid,
    },
}
