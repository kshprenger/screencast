use super::errors::VideoErrors;
use super::{Frame, ScreenCapturer};
use scap::{
    capturer::{Capturer, Options},
    Target,
};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

// Buffer size for frames (2 seconds at 60fps)
const FRAME_BUFFER_SIZE: usize = 120;

pub struct ScapCapturer {
    target: Target,
}

impl ScapCapturer {
    fn get_primary_display() -> Result<Target, VideoErrors> {
        let targets = scap::get_all_targets();
        for target in targets {
            if let Target::Display(_) = &target {
                return Ok(target);
            }
        }

        Err(VideoErrors::NoMonitorFound)
    }
}

impl ScreenCapturer for ScapCapturer {
    fn new() -> Result<Self, VideoErrors>
    where
        Self: Sized,
    {
        let target = Self::get_primary_display()?;
        Ok(Self { target })
    }

    fn start_capturing(&self) -> Result<mpsc::Receiver<Frame>, VideoErrors> {
        let (frame_tx, frame_rx) = mpsc::sync_channel::<Frame>(FRAME_BUFFER_SIZE);

        let target = self.target.clone();

        thread::spawn(move || {
            let options = Options {
                target: Some(target),
                fps: 60,
                show_cursor: true,
                show_highlight: false,
                excluded_targets: None,
                output_type: scap::frame::FrameType::BGRAFrame,
                output_resolution: scap::capturer::Resolution::_720p,
                crop_area: None,
            };

            let mut capturer = match Capturer::build(options) {
                Ok(capturer) => capturer,
                Err(e) => {
                    tracing::error!("Failed to create scap capturer: {}", e);
                    return;
                }
            };

            capturer.start_capture();

            tracing::info!("Starting scap capture loop");

            loop {
                match capturer.get_next_frame() {
                    Ok(frame) => {
                        let our_frame = match frame {
                            scap::frame::Frame::BGRA(bgra_frame) => Frame {
                                width: bgra_frame.width as u32,
                                height: bgra_frame.height as u32,
                                data: bgra_frame.data,
                            },
                            _ => unreachable!(),
                        };

                        if let Err(_) = frame_tx.send(our_frame) {
                            tracing::warn!("Frame receiver closed, stopping capture");
                            break;
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to capture frame: {}", e);

                        // Add small delay to prevent busy loop on persistent errors
                        thread::sleep(Duration::from_millis(16)); // ~60fps timing
                    }
                }
            }

            tracing::info!("Scap capture thread terminating");
        });

        Ok(frame_rx)
    }
}

impl Drop for ScapCapturer {
    fn drop(&mut self) {
        tracing::warn!("ScapCapturer dropped");
    }
}

#[cfg(test)]
mod tests {
    use tokio::time::Instant;

    use super::*;
    use std::time::Duration;

    #[test]
    fn test_scap_capturer_creation() {
        let capturer = ScapCapturer::new();
        assert!(capturer.is_ok(), "Should be able to create ScapCapturer");
    }

    #[test]
    fn test_scap_performance() {
        let capturer = ScapCapturer::new().unwrap();
        let frame_rx = capturer.start_capturing().unwrap();

        let start = Instant::now();
        let mut count = 0;
        let timeout = Duration::from_secs(3);

        while start.elapsed() < timeout {
            if let Ok(_frame) = frame_rx.recv_timeout(Duration::from_millis(50)) {
                count += 1;
            }
        }

        let elapsed = start.elapsed();
        let fps = count as f64 / elapsed.as_secs_f64();

        println!(
            "Scap performance test: {:.1} FPS over {:.2}s",
            fps,
            elapsed.as_secs_f64()
        );
        assert!(fps > 10.0, "Should capture at least 10 FPS, got {:.1}", fps);
    }

    #[test]
    fn test_frame_format() {
        let capturer = ScapCapturer::new().unwrap();
        let frame_rx = capturer.start_capturing().unwrap();

        if let Ok(frame) = frame_rx.recv_timeout(Duration::from_secs(2)) {
            assert!(frame.width > 0, "Frame should have positive width");
            assert!(frame.height > 0, "Frame should have positive height");
            assert_eq!(
                frame.data.len(),
                (frame.width * frame.height * 4) as usize,
                "BGRA data should be width * height * 4 bytes"
            );

            println!(
                "Frame format test passed: {}x{}, {} bytes",
                frame.width,
                frame.height,
                frame.data.len()
            );
        } else {
            panic!("Should receive at least one frame within 2 seconds");
        }
    }
}
