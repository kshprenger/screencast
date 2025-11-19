use std::hint::spin_loop;
use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use capture::ScreenCapturer;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;

use tokio::sync::Mutex;

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
                let bytes = Bytes::copy_from_slice(&buffer[..n]);
                tokio::select! {
                    _ = webrtc.send_buffer(&bytes) => {},
                    _ = ctx.cancelled() => {
                        tracing::warn!("Terminated sample sending task");
                        return;
                    }
                }
                sleep(Duration::from_millis(1)).await;
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
                WebrtcEvents::TrackArrived(track) => {
                    *self.state.lock().await = GUIState::Watching(track);
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
