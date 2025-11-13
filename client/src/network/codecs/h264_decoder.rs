use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::{mpsc, Arc};
use std::thread;

use bytes::Bytes;
use ffmpeg_next::format::Pixel;
use ffmpeg_next::software::scaling::{context::Context, flag::Flags};
use ffmpeg_next::{codec, frame, packet};

use crate::capture::{Frame, VideoErrors};

/// H264 decoder that converts WebRTC H264 samples to BGRA frames
///
/// This decoder receives H264-encoded data packets (typically from WebRTC)
/// and decodes them into BGRA frames that match the Frame format defined
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
///     // Process BGRA frame
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

    let mut scaler: Option<Context> = None;
    let mut packet_buffer = VecDeque::new();

    while *running.lock().unwrap() {
        match data_rx.recv() {
            Ok(data) => {
                packet_buffer.extend(data);
            }
            Err(e) => {
                tracing::error!("Error occured {e}, stopping decoder");
                break;
            }
        }

        // Process buffered data
        while !packet_buffer.is_empty() {
            // Try to extract a complete packet
            // For H264, we need to find NAL units. This is a simplified approach
            // that assumes each data chunk is a complete packet.
            let packet_data: Vec<u8> = packet_buffer.drain(..).collect();

            if packet_data.is_empty() {
                break;
            }

            // Create FFmpeg packet
            let packet = packet::Packet::copy(&packet_data);

            // Send packet to decoder
            match decoder.send_packet(&packet) {
                Ok(()) => {
                    // Try to receive decoded frames
                    loop {
                        let mut decoded_frame = frame::Video::empty();
                        match decoder.receive_frame(&mut decoded_frame) {
                            Ok(()) => {
                                // Convert the decoded frame to BGRA and send it
                                if let Err(e) =
                                    process_decoded_frame(&decoded_frame, &frame_tx, &mut scaler)
                                {
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
                    break;
                }
                Err(e) => {
                    tracing::error!("Error sending packet to decoder: {e}");
                    // Try to continue with next packet
                    break;
                }
            }
        }

        // Small sleep to avoid busy-waiting
        thread::sleep(std::time::Duration::from_millis(1));
    }

    tracing::info!("Decoder thread terminated");
    Ok(())
}

/// Process a decoded frame and convert it to BGRA format
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
            Pixel::BGRA,
            width,
            height,
            Flags::BILINEAR,
        )
        .ok();

        if scaler.is_none() {
            tracing::error!(
                "Failed to create scaling context for {}x{}  -> BGRA",
                width,
                height,
            );
            return Err(VideoErrors::CannotCapture);
        }
    }

    // Create output frame in BGRA format
    let mut bgra_frame = frame::Video::new(Pixel::BGRA, width, height);

    // Convert to BGRA
    if let Some(ref mut ctx) = scaler {
        ctx.run(decoded_frame, &mut bgra_frame)
            .map_err(|_| VideoErrors::CannotCapture)?;
    }

    // Extract BGRA data
    let bgra_data = bgra_frame.data(0);
    let stride = bgra_frame.stride(0);

    // Create Frame struct
    let mut frame_data = Vec::with_capacity((width * height * 4) as usize);

    // Copy data line by line to handle stride differences
    for y in 0..height {
        let line_start = (y as usize) * stride;
        let line_end = line_start + (width as usize * 4);
        if line_end <= bgra_data.len() {
            frame_data.extend_from_slice(&bgra_data[line_start..line_end]);
        } else {
            // Handle case where stride calculation doesn't match expected data
            tracing::error!(
                "Frame data size mismatch: expected line end {}, got buffer size {}",
                line_end,
                bgra_data.len()
            );
            return Err(VideoErrors::CannotCapture);
        }
    }

    let frame = Frame {
        width,
        height,
        data: frame_data,
    };

    // Send the frame
    frame_tx.send(frame).unwrap();

    Ok(())
}
