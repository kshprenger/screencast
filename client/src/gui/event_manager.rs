use std::{sync::Arc, time::Duration};

use capture::ScreenCapturer;
use webrtc::media::io::h264_writer::H264Writer;
use webrtc::media::{io::h264_reader::H264Reader, Sample};

use std::sync::Mutex;

use crate::network::H264Encoder;

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
}

impl GUIEventManager {
    async fn start_sending_frames(self: &Arc<Self>) {
        let frame_rx = match capture::XCapCapturer::new() {
            Err(err) => {
                tracing::error!("Could start stream: {err}");
                return;
            }
            Ok(capturer) => match capturer.start_capturing() {
                Err(err) => {
                    tracing::error!("Could start stream: {err}");
                    return;
                }
                Ok(frame_rx) => frame_rx,
            },
        };

        let h264_stream = H264Encoder::new(frame_rx).unwrap();
        let mut h264_reader = H264Reader::new(h264_stream, 1_048_576);
        let webrtc = Arc::clone(&self.webrtc);

        tokio::spawn(async move {
            loop {
                let nal = match h264_reader.next_nal() {
                    Ok(nal) => nal,
                    Err(err) => {
                        tracing::error!("All video frames parsed and sent: {err}");
                        break;
                    }
                };
                webrtc
                    .send_sample(&Sample {
                        data: nal.data.freeze(),
                        duration: Duration::from_secs(1),
                        ..Default::default()
                    })
                    .await;
            }
        });
    }

    async fn handle_events(self: Arc<Self>) {
        let mut events_rx = self.webrtc.subscribe().await;
        while let Some(event) = events_rx.recv().await {
            tracing::info!("Got event from network: {event}");
            match event {
                WebrtcEvents::GatheredAnswers => {
                    *self.state.lock().unwrap() = GUIState::Streaming;
                    self.start_sending_frames().await
                }
                WebrtcEvents::TrackArrived(track) => {
                    *self.state.lock().unwrap() = GUIState::Watching(track);
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
        });
        async_rt.spawn(Arc::clone(&manager).handle_events());
        manager
    }

    pub fn start_stream(self: &Arc<Self>) {
        self.async_rt
            .spawn(Arc::clone(&self.webrtc).create_and_send_offers());
    }

    pub fn stop_stream(self: &Arc<Self>) {
        *self.state.lock().unwrap() = GUIState::Idle;
        // conn.remove_track???
    }
}
