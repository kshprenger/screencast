use std::sync::{
    atomic::{AtomicPtr, Ordering},
    Arc,
};

use tokio::sync::mpsc;

use crate::{gui::GUIState, transport::WebrtcEvents};

pub(super) async fn handle_events(
    mut events_rx: mpsc::UnboundedReceiver<WebrtcEvents>,
    state: Arc<AtomicPtr<GUIState>>,
) {
    while let Some(event) = events_rx.recv().await {
        match event {
            WebrtcEvents::GatheredAnswers => {}
            WebrtcEvents::TrackArrived(track) => {
                // Ordering: Notify GUI loop about state changing
                state.store(
                    Box::into_raw(Box::new(GUIState::Watching(track))),
                    Ordering::Release,
                );
            }
        }
    }
}
