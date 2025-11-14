use std::collections::VecDeque;
use std::hint::spin_loop;
use std::io::{self, Read};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use ffmpeg_next::format::Pixel;
use ffmpeg_next::software::scaling::{context::Context, flag::Flags};
use ffmpeg_next::{codec, frame};

use crate::capture::{Frame, VideoErrors};

/// A wrapper that transforms a frame channel stream into a Read trait implementation
/// that provides H.264 encoded data on the fly.
///
/// # Color Space Conversion
///
/// The reader assumes input frames are in BGRA format (as provided by `XCapCapturer`).
/// It automatically converts to YUV420 (I420) format required by the H.264 encoder using
/// FFmpeg's high-performance scaling context.
///
/// # Threading
///
/// The encoder thread runs continuously, pulling frames from the receiver channel and
/// encoding them. Reading from the H264Encoder will block briefly if the buffer is empty,
/// allowing the encoder thread to catch up. The thread terminates when the frame channel
/// is closed.
pub struct H264Encoder {
    /// Buffer holding encoded data ready to be read
    buffer: Arc<Mutex<VecDeque<u8>>>,
    /// Flag to signal when the encoder thread is done
    done: Arc<Mutex<bool>>,
}

impl H264Encoder {
    pub fn new(frame_rx: mpsc::Receiver<Frame>) -> Result<Self, VideoErrors> {
        // Safe to call multiple times
        let _ = ffmpeg_next::init();

        let buffer = Arc::new(Mutex::new(VecDeque::new()));
        let done = Arc::new(Mutex::new(false));

        let buffer_clone = Arc::clone(&buffer);
        let done_clone = Arc::clone(&done);

        thread::spawn(move || {
            if let Err(e) = run_encoder(frame_rx, buffer_clone, done_clone) {
                tracing::error!("Encoder thread error: {e}");
            }
        });

        Ok(Self { buffer, done })
    }
}

impl Read for H264Encoder {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            let mut buffer = self.buffer.lock().unwrap();

            // If there's data in the buffer, copy it to the read buffer
            if !buffer.is_empty() {
                let bytes_to_copy = std::cmp::min(buf.len(), buffer.len());
                for i in 0..bytes_to_copy {
                    buf[i] = buffer.pop_front().unwrap();
                }
                return Ok(bytes_to_copy);
            }

            // Drop the lock before checking done status
            drop(buffer);

            // Check if we're done and buffer is empty
            let is_done = *self.done.lock().unwrap();
            if is_done {
                return Ok(0); // EOF
            }

            // Brief sleep to avoid busy-waiting
            thread::sleep(std::time::Duration::from_millis(1));
            spin_loop(); // Optimize
        }
    }
}

fn run_encoder(
    frame_rx: mpsc::Receiver<Frame>,
    buffer: Arc<Mutex<VecDeque<u8>>>,
    done: Arc<Mutex<bool>>,
) -> Result<(), VideoErrors> {
    let codec = codec::encoder::find(codec::Id::H264).unwrap();
    let mut video = codec::context::Context::new_with_codec(codec)
        .encoder()
        .video()
        .unwrap();

    video.set_width(1080);
    video.set_height(720);
    video.set_format(Pixel::YUV420P);
    video.set_frame_rate(Some((60, 1)));
    video.set_time_base((1, 30));

    let mut encoder = video.open().unwrap();

    let mut scaler: Option<Context> = None;
    let mut prev_width = 0;
    let mut prev_height = 0;

    while let Ok(frame_data) = frame_rx.recv() {
        let width = frame_data.width as u32;
        let height = frame_data.height as u32;

        // Reinitialize scaler if dimensions changed
        if scaler.is_none() || width != prev_width || height != prev_height {
            scaler = Context::get(
                Pixel::BGRA,
                width,
                height,
                Pixel::YUV420P,
                width,
                height,
                Flags::BILINEAR,
            )
            .ok();

            if scaler.is_none() {
                tracing::error!("Failed to create scaling context");
                continue;
            }

            prev_width = width;
            prev_height = height;

            // Update encoder dimensions if they changed
            encoder.set_width(width);
            encoder.set_height(height);
        }

        let mut input_frame = frame::Video::new(Pixel::BGRA, width, height);
        let mut output_frame = frame::Video::new(Pixel::YUV420P, width, height);

        // BGRA is 4 bytes per pixel
        let expected_size = (input_frame.width() * input_frame.height() * 4) as usize;

        let frame_data_slice = frame_data.data.as_slice();
        let input_frame_data = input_frame.data_mut(0);

        // Safe
        input_frame_data.copy_from_slice(&frame_data_slice);

        // Scale/convert the frame
        if let Some(ref mut ctx) = scaler {
            if ctx.run(&input_frame, &mut output_frame).is_err() {
                tracing::error!("Failed to scale frame");
                continue;
            }
        }

        // Encode the frame
        if encoder.send_frame(&output_frame).is_err() {
            tracing::error!("Failed to send frame to encoder");
            continue;
        }

        let mut encoded_packet = ffmpeg_next::packet::Packet::empty();
        while encoder.receive_packet(&mut encoded_packet).is_ok() {
            let data = encoded_packet.data().unwrap();
            let mut buf = buffer.lock().unwrap();
            buf.extend(data);
        }
    }

    tracing::warn!("Frame receiver closed, encoder thread terminated");
    *done.lock().unwrap() = true;
    Ok(())
}
