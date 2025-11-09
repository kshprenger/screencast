use std::sync::{atomic::Ordering, Arc};

use tokio::sync::{mpsc, Mutex};

use crate::{gui::GUIState, transport::WebrtcEvents};

pub(super) async fn handle_events(
    mut events_rx: mpsc::UnboundedReceiver<WebrtcEvents>,
    state: Arc<Mutex<GUIState>>,
) {
    while let Some(event) = events_rx.recv().await {
        tracing::debug!("Got event from webrtc: {event:?}");
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
