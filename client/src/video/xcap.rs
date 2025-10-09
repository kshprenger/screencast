use super::errors::VideoErrors;
use super::{Frame, ScreenCapturer, FRAME_BUFFER_SIZE};
use async_trait::async_trait;
use tokio::sync::mpsc;
use xcap::Monitor;

pub struct XCapCapturer {
    monitor: Monitor,
}

impl XCapCapturer {
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
}

#[async_trait]
impl ScreenCapturer for XCapCapturer {
    fn new() -> Result<Self, VideoErrors>
    where
        Self: Sized,
    {
        if let Some(monitor) = Self::get_primary_monitor() {
            Ok(Self { monitor })
        } else {
            Err(VideoErrors::NoMonitorError)
        }
    }

    async fn start_capturing(&self) -> Result<mpsc::Receiver<Frame>, VideoErrors> {
        match self.monitor.video_recorder() {
            Ok((_, xcap_frame_rx)) => {
                let (frame_tx, frame_rx) = mpsc::channel::<Frame>(FRAME_BUFFER_SIZE);
                tokio::task::spawn_blocking(move || loop {
                    match xcap_frame_rx.recv() {
                        Ok(frame) => {
                            let new_frame = Frame {
                                width: frame.width,
                                height: frame.height,
                                data: frame.raw,
                            };
                            if let Err(_) = frame_tx.blocking_send(new_frame) {
                                tracing::warn!("Frame rx is closed. Terminating task.");
                                return;
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
    }
}
