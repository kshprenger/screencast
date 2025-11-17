use std::sync::Arc;
use std::time::{Duration, Instant};

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
                let mut sample_builder = SampleBuilder::new(30, H264Packet::default(), 50000);
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

                // Batch RTP packets for 1 seconds before sorting and decoding
                let mut rtp_batch: Vec<(u16, webrtc::rtp::packet::Packet)> = Vec::new();
                let batch_timeout = Duration::from_secs(1);
                let mut batch_start = Instant::now();

                while let Ok((rtp_packet, _)) = track.read_rtp().await {
                    let seq_num = rtp_packet.header.sequence_number;
                    rtp_batch.push((seq_num, rtp_packet));

                    if batch_start.elapsed() >= batch_timeout && !rtp_batch.is_empty() {
                        rtp_batch.sort_by_key(|(seq, _)| *seq);

                        tracing::debug!(
                            "Processing batch of {} RTP packets, sorted by sequence number",
                            rtp_batch.len()
                        );

                        for (seq, rtp_packet) in rtp_batch.drain(..) {
                            tracing::debug!("Processing RTP packet with sequence number: {}", seq);
                            sample_builder.push(rtp_packet);
                            while let Some(sample) = sample_builder.pop() {
                                decoder.feed_data(sample.data).unwrap();
                            }
                        }

                        batch_start = Instant::now();
                    }
                }

                if !rtp_batch.is_empty() {
                    rtp_batch.sort_by_key(|(seq, _)| *seq);
                    tracing::debug!("Processing final batch of {} RTP packets", rtp_batch.len());

                    for (seq, rtp_packet) in rtp_batch {
                        tracing::debug!(
                            "Processing final RTP packet with sequence number: {}",
                            seq
                        );
                        sample_builder.push(rtp_packet);
                        while let Some(sample) = sample_builder.pop() {
                            decoder.feed_data(sample.data).unwrap();
                        }
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
