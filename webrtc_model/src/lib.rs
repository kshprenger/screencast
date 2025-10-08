use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SDP {
    sdp: String,
    #[serde(rename = "type")]
    type_: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct IceCandidate {
    candidate: String,
    sdp_m_line_index: u32,
    sdp_mid: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
pub enum SignalingMessage {
    Offer { sdp: SDP },
    Answer { sdp: SDP },
    IceCandidate { ice_candidate: IceCandidate },
    NewPeer { peer_id: Uuid },
    PeerLeft { peer_id: Uuid },
}
