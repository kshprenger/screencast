use std::sync::Arc;

use tokio::sync::{mpsc, Mutex};

use crate::{gui::GUIState, network::WebrtcEvents};

pub(super) async fn handle_events(
    mut events_rx: mpsc::UnboundedReceiver<WebrtcEvents>,
    state: Arc<Mutex<GUIState>>,
) {
    while let Some(event) = events_rx.recv().await {
        tracing::info!("Got event from network: {event}");
        let mut state = state.lock().await;
        match event {
            WebrtcEvents::GatheredAnswers => {
                *state = GUIState::Streaming;
            }
            WebrtcEvents::TrackArrived(track) => {
                *state = GUIState::Watching(track);
            }
        }
    }
    tracing::warn!("Event channel from webrtc was closed")
}
