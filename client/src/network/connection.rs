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
                    // let feedable_reader = FeedableReader::new();
                    // let feedable_reader_clone = feedable_reader.clone();
                    // let mut nal_builder = H264Reader::new(feedable_reader, 1_048_576_00);
                    let mut buf = Vec::new();
                    let separator = [0x00, 0x00, 0x00, 0x01];
                    loop {
                        let data = data_rx.blocking_recv().unwrap();
                        buf.extend_from_slice(&data);

                        let mut separator_positions = Vec::new();
                        for i in 0..buf.len().saturating_sub(3) {
                            if &buf[i..i + 4] == separator {
                                separator_positions.push(i);
                            }
                        }

                        if separator_positions.len() >= 2 {
                            let start = separator_positions[0];
                            let end = separator_positions[1];
                            tracing::info!("NAL UNIT WITH SIZE: {}", end - start);
                            let nal_unit = &buf[start..end];
                            decoder
                                .feed_data(Bytes::copy_from_slice(&nal_unit))
                                .unwrap();
                            buf.drain(..end);
                        }

                        // feedable_reader_clone.feed(&data);
                        // while let Ok(nal) = nal_builder.next_nal() {
                        //     let nal_type = nal.data[0] & 0x1f;
                        //     let nal_type_name = match nal_type {
                        //         1 => "Slice (non-IDR)",
                        //         5 => "Slice (IDR/Keyframe)",
                        //         6 => "SEI",
                        //         7 => "SPS",
                        //         8 => "PPS",
                        //         9 => "AUD",
                        //         _ => "Other",
                        //     };

                        //     tracing::info!(
                        //         "NAL type: {} ({}), size: {} bytes",
                        //         nal_type,
                        //         nal_type_name,
                        //         nal.data.len()
                        //     );

                        //     tracing::info!("Before extension: {:02x?}", &nal.data.as_bytes()[..6]);

                        //     let mut nal_with_start = Vec::new();
                        //     if !nal.data.starts_with(&[0x00, 0x00, 0x00, 0x01]) {
                        //         if nal_type != 7 && nal_type != 8 && nal_type != 5 {
                        //             continue;
                        //         }
                        //         nal_with_start.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
                        //     }

                        //     nal_with_start.extend_from_slice(&nal.data);
                        //     tracing::info!("After extension: {:02x?}", &nal_with_start[..8]);
                        //     decoder
                        //         .feed_data(Bytes::copy_from_slice(&nal_with_start))
                        //         .unwrap();
                        // }
                        // decoder.feed_data(Bytes::copy_from_slice(&data)).unwrap();
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
