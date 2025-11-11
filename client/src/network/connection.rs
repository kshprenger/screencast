use std::sync::{mpsc, Arc};

use uuid::Uuid;
use webrtc::{peer_connection::RTCPeerConnection, rtp::packet::Packet};
use webrtc_model::{RoutedSignallingMessage, Routing, SignallingMessage};

use crate::network::{H264Decoder, WebrtcEvents, WebrtcNetwork};

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

            let (frame_tx, frame_rx) = mpsc::channel();
            let (packet_tx, packet_rx) = mpsc::channel::<Packet>();

            std::thread::spawn(move || {
                let mut decoder = H264Decoder::new();
                match decoder.decode_rtp_packet(&packet_rx.recv().unwrap().payload) {
                    Ok(Some(frame)) => {
                        if let Err(err) = frame_tx.send(frame) {
                            tracing::error!("Could not send decoded frame to GUI: {err}");
                            return;
                        }
                    }
                    Ok(None) => {
                        // More data needed, continue accumulating
                        tracing::trace!("Awaiting more RTP packets to complete frame");
                    }
                    Err(err) => {
                        tracing::error!("H.264 decoding error: {err}");
                    }
                }
            });

            Box::pin(async move {
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
                    // Decode H.264 RTP packet payload to RGBA frame
                    packet_tx.send(rtp_packet);
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
