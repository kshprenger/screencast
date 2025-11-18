use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::{Buf, Bytes};
use image::EncodableLayout;
use tokio::sync::{mpsc, Mutex};
use uuid::Uuid;
use webrtc::media::io::h264_reader::{H264Reader, NalUnitType};
use webrtc::{
    media::io::sample_builder::SampleBuilder, peer_connection::RTCPeerConnection,
    rtp::codecs::h264::H264Packet,
};
use webrtc_model::{RoutedSignallingMessage, Routing, SignallingMessage};

use crate::network::reader::FeedableReader;
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

    async fn setup_on_data_channel(self: &Arc<Self>, conn: &RTCPeerConnection) {
        let self_clone1 = Arc::clone(&self);
        conn.on_data_channel(Box::new(move |data_channel| {
            let self_clone2 = Arc::clone(&self_clone1);
            tracing::info!("Received remote track");
            Box::pin(async move {
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

                let (data_tx, mut data_rx) = mpsc::unbounded_channel::<Vec<u8>>();

                std::thread::spawn(move || {
                    let feedable_reader = FeedableReader::new();
                    let feedable_reader_clone = feedable_reader.clone();
                    let mut nal_builder = H264Reader::new(feedable_reader, 60000);
                    loop {
                        let data = data_rx.blocking_recv().unwrap();
                        feedable_reader_clone.feed(&data);
                        while let Ok(nal) = nal_builder.next_nal() {
                            let mut nal_with_start = Vec::new();
                            nal_with_start.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
                            nal_with_start.extend_from_slice(&nal.data);
                            decoder
                                .feed_data(Bytes::copy_from_slice(&nal_with_start))
                                .unwrap();
                        }
                    }
                });

                data_channel.on_open(Box::new(|| {
                    tracing::info!("Data channel opened");
                    Box::pin(async {})
                }));

                data_channel.on_close(Box::new(|| {
                    tracing::info!("Data channel closed");
                    Box::pin(async {})
                }));

                data_channel.on_message(Box::new(move |message| {
                    let data_tx_clone = data_tx.clone();
                    Box::pin(async move {
                        data_tx_clone.clone().send(message.data.to_vec()).unwrap();
                    })
                }));
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
        self.setup_on_data_channel(&conn).await;

        conn
    }
}
