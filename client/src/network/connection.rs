use std::sync::{mpsc, Arc};

use uuid::Uuid;
use webrtc::{
    media::io::sample_builder::SampleBuilder, peer_connection::RTCPeerConnection,
    rtp::codecs::h264::H264Packet,
};
use webrtc_model::{RoutedSignallingMessage, Routing, SignallingMessage};

use crate::network::{codecs, WebrtcEvents, WebrtcNetwork};

impl WebrtcNetwork {
    async fn setup_on_ice_candidate(self: &Arc<Self>, conn: &RTCPeerConnection, to: Uuid) {
        let self_clone1 = Arc::clone(self);
        conn.on_ice_candidate(Box::new(move |candidate| {
            let self_clone2 = Arc::clone(&self_clone1);
            Box::pin(async move {
                if let Some(candidate) = candidate {
                    tracing::info!("Found ICE candidate: {candidate}");
                    let mut conns_state = self_clone2.conns_state.lock().await;
                    if let Err(err) = Self::send_signalling_message(
                        &mut conns_state,
                        RoutedSignallingMessage {
                            routing: Routing::To(to),
                            message: SignallingMessage::ICECandidate { candidate },
                        },
                    )
                    .await
                    {
                        tracing::error!("Could not send ICE candidate to peer: {to}, error: {err}");
                    }
                } else {
                    tracing::info!("ICE gathering completed");
                }
            })
        }));
    }

    async fn setup_on_track(self: &Arc<Self>, conn: &RTCPeerConnection) {
        let self_clone1 = Arc::clone(&self);
        conn.on_track(Box::new(move |track, _, _| {
            let self_clone2 = Arc::clone(&self_clone1);
            tracing::info!("Received remote track");

            Box::pin(async move {
                let mut sample_builder = SampleBuilder::new(90, H264Packet::default(), 90000);
                let (decoder, frame_rx) = codecs::H264Decoder::start_decoding().unwrap();
                self_clone2
                    .conns_state
                    .lock()
                    .await
                    .events_tx
                    .as_ref()
                    .and_then(|chan| {
                        // Notify GUI about stream with decoded frame channel
                        if let Err(err) = chan.send(WebrtcEvents::TrackArrived(frame_rx)) {
                            tracing::error!("Could not send track event to GUI: {err}");
                        }
                        Some(())
                    })
                    .or_else(|| {
                        tracing::warn!("No GUI event subscription");
                        Some(())
                    });

                while let Ok((rtp_packet, _)) = track.read_rtp().await {
                    sample_builder.push(rtp_packet);
                    while let Some(sample) = sample_builder.pop() {
                        decoder.feed_data(sample.data).unwrap()
                    }
                }
                tracing::warn!("Track ended for peer");
            })
        }));
    }

    pub(super) async fn create_connection(self: &Arc<Self>, from: Uuid) -> RTCPeerConnection {
        let conn = self
            .webrtc_api
            .new_peer_connection(self.default_config.clone())
            .await
            .unwrap(); // Safety: Valid config was provided

        self.setup_on_ice_candidate(&conn, from).await;
        self.setup_on_track(&conn).await;

        conn
    }
}
