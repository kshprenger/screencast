use std::collections::VecDeque;
use std::io::{self, Read};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use ffmpeg_next::format::Pixel;
use ffmpeg_next::software::scaling::{context::Context, flag::Flags};
use ffmpeg_next::{codec, frame};

use crate::capture::Frame;
use crate::codecs::errors::CodecErrors;

pub struct H264Encoder {
    buffer: Arc<Mutex<VecDeque<u8>>>,
    done: Arc<Mutex<bool>>,
}

impl H264Encoder {
    pub fn new(frame_rx: mpsc::Receiver<Frame>) -> Result<Self, CodecErrors> {
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
        let mut buffer = self.buffer.lock().unwrap();
        if !buffer.is_empty() {
            let bytes_to_copy = std::cmp::min(buf.len(), buffer.len());
            for i in 0..bytes_to_copy {
                buf[i] = buffer.pop_front().unwrap();
            }
            return Ok(bytes_to_copy);
        }

        drop(buffer);

        let is_done = *self.done.lock().unwrap();
        if is_done {
            return Ok(0); // EOF
        }

        Ok(0)
    }
}

fn run_encoder(
    frame_rx: mpsc::Receiver<Frame>,
    buffer: Arc<Mutex<VecDeque<u8>>>,
    done: Arc<Mutex<bool>>,
) -> Result<(), CodecErrors> {
    let codec = codec::encoder::find(codec::Id::H264).expect("No h264 codec");
    let mut video = codec::context::Context::new_with_codec(codec)
        .encoder()
        .video()
        .unwrap(); // Safe because codec is present

    video.set_width(1920);
    video.set_height(1080);
    video.set_format(Pixel::YUV420P);
    video.set_frame_rate(Some((60, 1)));
    video.set_time_base((1, 60));

    // low-latency streaming
    video.set_gop(60); // GOP size = 1 second at 60fps
    video.set_max_b_frames(0); // Disable B-frames for lowest latency

    // low-latency options
    let mut encoder = video
        .open_as_with(
            codec,
            vec![
                ("preset", "ultrafast"),
                ("tune", "zerolatency"),
                ("x264-params", "bframes=0:force-cfr=1"),
            ]
            .into_iter()
            .collect(),
        )
        .unwrap();

    let mut scaler: Option<Context> = None;
    let mut prev_width = 0;
    let mut prev_height = 0;
    let mut frame_num = 0;

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
                1920,
                1080,
                Flags::FAST_BILINEAR,
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
        let mut output_frame = frame::Video::new(Pixel::YUV420P, 1920, 1080);

        let frame_data_slice = frame_data.data.as_slice();
        let input_frame_data = input_frame.data_mut(0);

        // Safe, because data already align as avbuffer format in scap capturer
        input_frame_data.copy_from_slice(&frame_data_slice);

        if let Some(ref mut ctx) = scaler {
            if ctx.run(&input_frame, &mut output_frame).is_err() {
                tracing::error!("Failed to scale frame");
                continue;
            }
        }

        output_frame.set_pts(Some(frame_num));
        frame_num += 1;

        if encoder.send_frame(&output_frame).is_err() {
            tracing::error!("Failed to send frame to encoder");
            continue;
        }

        let mut encoded_packet = ffmpeg_next::packet::Packet::empty();
        while encoder.receive_packet(&mut encoded_packet).is_ok() {
            let data = encoded_packet.data().unwrap(); // Sage because recv is ok
            let mut buf = buffer.lock().unwrap();
            buf.extend(data);
        }
    }

    tracing::warn!("Frame receiver closed, encoder thread terminated");
    *done.lock().unwrap() = true;
    Ok(())
}
