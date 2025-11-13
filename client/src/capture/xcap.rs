use super::errors::VideoErrors;
use super::{Frame, ScreenCapturer};
use std::sync::mpsc;
use xcap::Monitor;

// 10 seconds in 60 fps
const FRAME_BUFFER_SIZE: usize = 10 * 60;

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

impl ScreenCapturer for XCapCapturer {
    fn new() -> Result<Self, VideoErrors>
    where
        Self: Sized,
    {
        if let Some(monitor) = Self::get_primary_monitor() {
            Ok(Self { monitor })
        } else {
            Err(VideoErrors::NoMonitorFound)
        }
    }

    fn start_capturing(&self) -> Result<mpsc::Receiver<Frame>, VideoErrors> {
        match self.monitor.video_recorder() {
            Ok((recorder, xcap_frame_rx)) => {
                let (frame_tx, frame_rx) = mpsc::sync_channel::<Frame>(FRAME_BUFFER_SIZE);
                recorder.start().unwrap();

                // Magic trick
                // FIXME
                std::mem::forget(recorder);

                std::thread::spawn(move || loop {
                    match xcap_frame_rx.recv() {
                        Ok(frame) => {
                            tracing::info!("Got frame from xcap");
                            let new_frame = Frame {
                                width: frame.width,
                                height: frame.height,
                                data: frame.raw,
                            };
                            if let Err(_) = frame_tx.send(new_frame) {
                                tracing::warn!("Frame rx is closed. Terminating task.");
                                return;
                            }
                        }
                        Err(_) => {
                            tracing::warn!("No frame");
                        }
                    }
                });
                return Ok(frame_rx);
            }
            Err(err) => {
                tracing::error!("Could not start video capturing: {err}");
                return Err(VideoErrors::CannotCapture);
            }
        }
    }
}
