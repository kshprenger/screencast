mod errors;

use errors::VideoErrors;
use tokio::sync::mpsc;
use xcap::Monitor;

// 10 seconds in 60 fps
const FRAME_BUFFER_SIZE: usize = 10 * 60;

fn get_primary_monitor() -> Option<Monitor> {
    match xcap::Monitor::all() {
        Ok(monitors) => {
            if let Some(monitor) = monitors
                .into_iter()
                .find(|m| m.is_primary().unwrap_or(false))
            {
                return Some(monitor);
            } else {
                tracing::error!("Could not determine primary monitor");
                return None;
            }
        }
        Err(err) => {
            tracing::error!("Could not determine monitors: {err}");
            return None;
        }
    }
}

// We need to make this function async in order to
// cross boundary between thread-based screen capturing
// and async webrtc transport. Task with cpu-bound operation will be blocking.
pub(super) async fn start_screen_recording() -> Result<mpsc::Receiver<xcap::Frame>, VideoErrors> {
    if let Some(monitor) = get_primary_monitor() {
        match monitor.video_recorder() {
            Ok((_, xcap_frame_rx)) => {
                let (frame_tx, frame_rx) = mpsc::channel::<xcap::Frame>(FRAME_BUFFER_SIZE);
                tokio::task::spawn_blocking(async move || loop {
                    match xcap_frame_rx.recv() {
                        Ok(frame) => {
                            if let Err(_) = frame_tx.send(frame).await {
                                tracing::warn!("Frame rx is closed. Terminating task.")
                            }
                        }
                        Err(_) => {
                            tracing::warn!("Frame tx is closed. Terminating task.");
                            return;
                        }
                    }
                });
                return Ok(frame_rx);
            }
            Err(err) => {
                tracing::error!("Could not start video capturing: {err}");
                return Err(VideoErrors::RecordError);
            }
        }
    } else {
        return Err(VideoErrors::NoMonitorError);
    }
}
