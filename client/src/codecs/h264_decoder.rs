use std::hint::spin_loop;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use bytes::Bytes;
use ffmpeg_next::format::Pixel;
use ffmpeg_next::software::scaling::{context::Context, flag::Flags};
use ffmpeg_next::{codec, frame, packet};

use crate::capture::{Frame, VideoErrors};
use crate::codecs::errors::CodecErrors;

#[derive(Clone)]
pub struct H264Decoder {
    data_tx: mpsc::Sender<Bytes>,
    running: Arc<Mutex<bool>>,
}

impl H264Decoder {
    pub fn start_decoding(dims: (u32, u32)) -> Result<(Self, mpsc::Receiver<Frame>), CodecErrors> {
        // Safe to call multiple times
        let _ = ffmpeg_next::init();

        let (frame_tx, frame_rx) = mpsc::channel();
        let (data_tx, data_rx) = mpsc::channel();

        let running = Arc::new(Mutex::new(true));
        let running_clone = Arc::clone(&running);
        *running.lock().unwrap() = true;

        thread::spawn(move || {
            if let Err(e) = run_decoder(data_rx, frame_tx, running_clone, dims) {
                tracing::error!("Decoder thread error: {e}");
            }
        });

        Ok((Self { data_tx, running }, frame_rx))
    }

    pub fn feed_data(&self, data: Bytes) -> Result<(), CodecErrors> {
        self.data_tx.send(data).unwrap(); // Safe
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
    dims: (u32, u32),
) -> Result<(), VideoErrors> {
    let codec = codec::decoder::find(codec::Id::H264).expect("No h264 codec");

    let mut decoder = codec::context::Context::new_with_codec(codec)
        .decoder()
        .video()
        .unwrap(); // Safe, because codec is present

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
                                        dims,
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
                        tracing::warn!("Decoder needs more data");
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
    dims: (u32, u32),
) -> Result<(), CodecErrors> {
    let width = decoded_frame.width();
    let height = decoded_frame.height();
    let input_format = decoded_frame.format();

    let (out_width, out_height) = dims;

    // Create or update scaler if needed
    if scaler.is_none()
        || scaler.as_ref().unwrap().input().width != width // Safe: None check first
        || scaler.as_ref().unwrap().input().height != height
        || scaler.as_ref().unwrap().output().width != out_width
        || scaler.as_ref().unwrap().output().height != out_height
    {
        *scaler = Context::get(
            input_format,
            width,
            height,
            Pixel::RGBA,
            out_width,
            out_height,
            Flags::FAST_BILINEAR,
        )
        .ok();

        if scaler.is_none() {
            tracing::error!(
                "Failed to create scaling context for {}x{}  -> RGB",
                width,
                height,
            );
            return Err(CodecErrors::CannotDecode);
        }
    }

    // Create output frame in RGB format
    let mut bgra_frame = frame::Video::new(Pixel::RGBA, out_width, out_height);

    // Convert to RGB
    if let Some(ref mut ctx) = scaler {
        ctx.run(decoded_frame, &mut bgra_frame).map_err(|err| {
            tracing::error!("{err}");
            return CodecErrors::CannotDecode;
        })?;
    }

    let frame = Frame {
        width: out_width,
        height: out_height,
        data: bgra_frame.data(0).to_vec(),
    };

    frame_tx.send(frame).unwrap(); // Safe

    Ok(())
}
