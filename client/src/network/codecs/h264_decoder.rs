use std::hint::spin_loop;
use std::sync::Mutex;
use std::sync::{mpsc, Arc};
use std::thread;

use bytes::Bytes;
use ffmpeg_next::format::Pixel;
use ffmpeg_next::software::scaling::{context::Context, flag::Flags};
use ffmpeg_next::{codec, frame, packet};

use crate::capture::{Frame, VideoErrors};

/// H264 decoder that converts WebRTC H264 samples to RGB frames
///
/// This decoder receives H264-encoded data packets (typically from WebRTC)
/// and decodes them into RGB frames that match the Frame format defined
/// in the capture module.
///
/// # Usage
///
/// ```rust
/// let decoder = H264Decoder::new()?;
/// let frame_rx = decoder.start_decoding();
///
/// // Feed H264 data to the decoder
/// decoder.feed_data(h264_bytes)?;
///
/// // Receive decoded frames
/// while let Ok(frame) = frame_rx.recv() {
///     // Process RGB frame
/// }
/// ```
pub struct H264Decoder {
    /// Channel sender for feeding H264 data to the decoder thread
    data_tx: mpsc::Sender<Bytes>,
    /// Internal state to track if decoder is running
    running: Arc<Mutex<bool>>,
}

impl H264Decoder {
    pub fn start_decoding() -> Result<(Self, mpsc::Receiver<Frame>), VideoErrors> {
        // Safe to call multiple times
        let _ = ffmpeg_next::init();

        let (frame_tx, frame_rx) = mpsc::channel();
        let (data_tx, data_rx) = mpsc::channel();

        let running = Arc::new(Mutex::new(true));
        let running_clone = Arc::clone(&running);
        *running.lock().unwrap() = true;

        thread::spawn(move || {
            if let Err(e) = run_decoder(data_rx, frame_tx, running_clone) {
                tracing::error!("Decoder thread error: {e}");
            }
        });

        Ok((Self { data_tx, running }, frame_rx))
    }

    pub fn feed_data(&self, data: Bytes) -> Result<(), VideoErrors> {
        self.data_tx.send(data).unwrap();
        Ok(())
    }

    /// Stop the decoder and cleanup resources
    pub fn stop(&self) {
        *self.running.lock().unwrap() = false;
    }
}

impl Drop for H264Decoder {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run_decoder(
    data_rx: mpsc::Receiver<Bytes>,
    frame_tx: mpsc::Sender<Frame>,
    running: Arc<Mutex<bool>>,
) -> Result<(), VideoErrors> {
    let codec = codec::decoder::find(codec::Id::H264).unwrap();

    let mut decoder = codec::context::Context::new_with_codec(codec)
        .decoder()
        .video()
        .unwrap();

    // Set low-delay flag
    decoder.set_flags(ffmpeg_next::codec::Flags::LOW_DELAY);

    let mut scaler: Option<Context> = None;

    while *running.lock().unwrap() {
        match data_rx.recv() {
            Ok(data) => {
                if data.is_empty() {
                    continue;
                }

                let packet = packet::Packet::copy(&data);

                // Send packet to decoder
                match decoder.send_packet(&packet) {
                    Ok(()) => {
                        // Try to receive decoded frames
                        loop {
                            let mut decoded_frame = frame::Video::empty();
                            match decoder.receive_frame(&mut decoded_frame) {
                                Ok(()) => {
                                    if let Err(e) = process_decoded_frame(
                                        &decoded_frame,
                                        &frame_tx,
                                        &mut scaler,
                                    ) {
                                        tracing::error!("Failed to process decoded frame: {e}");
                                    }
                                }
                                Err(ffmpeg_next::Error::Other {
                                    errno: ffmpeg_next::error::EAGAIN,
                                }) => {
                                    // No more frames available right now
                                    break;
                                }
                                Err(e) => {
                                    tracing::error!("Error receiving frame from decoder: {e}");
                                    break;
                                }
                            }
                        }
                    }
                    Err(ffmpeg_next::Error::Other {
                        errno: ffmpeg_next::error::EAGAIN,
                    }) => {
                        // Decoder needs more data
                        tracing::warn!("Decoder needs more data, but we are not buffering. This might be an issue.");
                        break;
                    }
                    Err(e) => {
                        tracing::error!("Error sending packet to decoder: {e}");
                    }
                }
            }
            Err(e) => {
                tracing::error!("Error occured {e}, stopping decoder");
                break;
            }
        }
        spin_loop();
    }

    tracing::info!("Decoder thread terminated");
    Ok(())
}

/// Process a decoded frame and convert it to RGB format
fn process_decoded_frame(
    decoded_frame: &frame::Video,
    frame_tx: &mpsc::Sender<Frame>,
    scaler: &mut Option<Context>,
) -> Result<(), VideoErrors> {
    let width = decoded_frame.width();
    let height = decoded_frame.height();
    let input_format = decoded_frame.format();

    // Create or update scaler if needed
    if scaler.is_none()
        || scaler.as_ref().unwrap().input().width != width
        || scaler.as_ref().unwrap().input().height != height
    {
        *scaler = Context::get(
            input_format,
            width,
            height,
            Pixel::RGBA,
            1080,
            720,
            Flags::FAST_BILINEAR,
        )
        .ok();

        if scaler.is_none() {
            tracing::error!(
                "Failed to create scaling context for {}x{}  -> RGB",
                width,
                height,
            );
            return Err(VideoErrors::CannotCapture);
        }
    }

    // Create output frame in RGB format
    let mut bgra_frame = frame::Video::new(Pixel::RGBA, 1080, 720);

    // Convert to RGB
    if let Some(ref mut ctx) = scaler {
        ctx.run(decoded_frame, &mut bgra_frame).map_err(|err| {
            tracing::error!("{err}");
            VideoErrors::CannotCapture
        })?;
    }

    let frame = Frame {
        width: 1080,
        height: 720,
        data: bgra_frame.data(0).to_vec(),
    };

    frame_tx.send(frame).unwrap();

    Ok(())
}
