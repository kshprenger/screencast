use std::hint::spin_loop;
use std::io::Read;
use std::sync::Arc;

use bytes::Bytes;
use capture::ScreenCapturer;
use tokio_util::sync::CancellationToken;

use tokio::sync::Mutex;

use crate::codecs::{self, H264Decoder, H264Encoder};

use crate::{
    capture::{self},
    gui::GUIState,
    network::{WebrtcEvents, WebrtcNetwork},
};

// Border between sync GUI and async webrtc network
// Something like js in html & js relationship, where html is GUI
pub struct GUIEventManager {
    webrtc: Arc<WebrtcNetwork>,
    async_rt: Arc<tokio::runtime::Runtime>,
    state: Arc<Mutex<GUIState>>,
    decoder: Arc<Mutex<Option<H264Decoder>>>,
}

impl GUIEventManager {
    async fn start_sending_frames(self: &Arc<Self>, ctx: CancellationToken) {
        let frame_rx = match capture::ScapCapturer::new() {
            Err(err) => {
                tracing::error!("Could start stream: {err}");
                return;
            }
            Ok(capturer) => match capturer.start_capturing(ctx.clone()) {
                Err(err) => {
                    tracing::error!("Could start stream: {err}");
                    return;
                }
                Ok(frame_rx) => frame_rx,
            },
        };

        let mut h264_stream = H264Encoder::new(frame_rx).unwrap();

        let webrtc = Arc::clone(&self.webrtc);

        tokio::spawn(async move {
            loop {
                let mut buffer = [0; 16384]; // This is upper bound size for webrtc on_message message
                let n = h264_stream.read(&mut buffer).unwrap();
                if n == 0 {
                    continue;
                }
                let bytes = Bytes::copy_from_slice(&buffer[..n]);
                tokio::select! {
                    _ = webrtc.send_buffer(&bytes) => {},
                    _ = ctx.cancelled() => {
                        tracing::warn!("Terminated sample sending task");
                        return;
                    }
                }
                spin_loop();
            }
        });
    }
    async fn handle_events(self: Arc<Self>) {
        let mut events_rx = self.webrtc.subscribe().await;
        let mut ctx = CancellationToken::new();

        while let Some(event) = events_rx.recv().await {
            tracing::info!("Got event from network: {event}");
            match event {
                WebrtcEvents::GatheredAnswers => {
                    *self.state.lock().await = GUIState::Streaming;
                    self.start_sending_frames(ctx.clone()).await
                }
                WebrtcEvents::TrackArrived(mut data_rx) => {
                    let (decoder, frame_rx) =
                        codecs::H264Decoder::start_decoding((1080, 720)).unwrap();
                    *self.decoder.lock().await = Some(decoder.clone());
                    std::thread::spawn(move || {
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
                                let nal_unit = &buf[start..end];
                                decoder
                                    .feed_data(Bytes::copy_from_slice(&nal_unit))
                                    .unwrap(); // Valid nal unit provided
                                buf.drain(..end);
                            }
                        }
                    });

                    *self.state.lock().await = GUIState::Watching(frame_rx);
                }
                WebrtcEvents::TrackRemoved => {
                    *self.state.lock().await = GUIState::Idle;
                    ctx.cancel();
                    ctx = CancellationToken::new();
                }
            }
        }
        tracing::warn!("Event channel from webrtc was closed")
    }
}

impl GUIEventManager {
    pub fn new_shared(
        webrtc: Arc<WebrtcNetwork>,
        async_rt: Arc<tokio::runtime::Runtime>,
        state: Arc<Mutex<GUIState>>,
    ) -> Arc<Self> {
        let manager = Arc::new(Self {
            webrtc,
            async_rt: Arc::clone(&async_rt),
            state,
            decoder: Arc::new(Mutex::new(None)),
        });
        async_rt.spawn(Arc::clone(&manager).handle_events());
        manager
    }

    pub fn start_stream(self: &Arc<Self>) {
        self.async_rt
            .spawn(Arc::clone(&self.webrtc).create_and_send_offers());
    }

    pub fn stop_stream(self: &Arc<Self>) {
        self.async_rt.spawn(Arc::clone(&self.webrtc).remove_track());
    }
}
