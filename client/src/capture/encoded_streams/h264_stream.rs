use std::collections::VecDeque;
use std::hint::spin_loop;
use std::io::{self, Read};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use openh264::encoder::Encoder;
use openh264::formats::YUVBuffer;

use crate::capture::{Frame, VideoErrors};

/// A wrapper that transforms a frame channel stream into a Read trait implementation
/// that provides H.264 encoded data on the fly.
///
/// # Color Space Conversion
///
/// The reader assumes input frames are in BGRA format (as provided by `XCapCapturer`).
/// It automatically converts to YUV420 (I420) format required by the H.264 encoder using
/// BT.601 color space conversion with 4:2:0 chroma subsampling.
///
/// # Threading
///
/// The encoder thread runs continuously, pulling frames from the receiver channel and
/// encoding them. Reading from the H264Stream will block briefly if the buffer is empty,
/// allowing the encoder thread to catch up. The thread terminates when the frame channel
/// is closed.
pub struct H264Stream {
    /// Buffer holding encoded data ready to be read
    buffer: Arc<Mutex<VecDeque<u8>>>,
    /// Flag to signal when the encoder thread is done
    done: Arc<Mutex<bool>>,
    /// Handle to the encoder thread
    _encoder_thread: Option<thread::JoinHandle<()>>,
}

impl H264Stream {
    pub fn new(frame_rx: mpsc::Receiver<Frame>) -> Result<Self, VideoErrors> {
        let buffer = Arc::new(Mutex::new(VecDeque::new()));
        let done = Arc::new(Mutex::new(false));

        let buffer_clone = Arc::clone(&buffer);
        let done_clone = Arc::clone(&done);

        // Spawn the encoder thread
        let encoder_thread = thread::spawn(move || {
            // Initialize the encoder
            let mut encoder = match Encoder::new() {
                Ok(enc) => enc,
                Err(err) => {
                    tracing::error!("Failed to initialize H.264 encoder: {}", err);
                    return;
                }
            };

            while let Ok(frame) = frame_rx.recv() {
                // Convert BGRA/RGBA frame to YUV420 for encoding
                let yuv_frame = match convert_to_yuv420(&frame) {
                    Ok(yuv) => yuv,
                    Err(err) => {
                        tracing::error!("Failed to convert frame to YUV420: {}", err);
                        continue;
                    }
                };

                // Encode the frame
                match encoder.encode(&yuv_frame) {
                    Ok(bitstream) => {
                        // Extract all NAL units from the encoded bitstream
                        let num_layers = bitstream.num_layers();
                        for layer_idx in 0..num_layers {
                            if let Some(layer) = bitstream.layer(layer_idx) {
                                let num_nals = layer.nal_count();
                                for nal_idx in 0..num_nals {
                                    if let Some(nal_unit_data) = layer.nal_unit(nal_idx) {
                                        let mut buf = buffer_clone.lock().unwrap();
                                        buf.extend(nal_unit_data.iter().cloned());
                                    }
                                }
                            }
                        }
                    }
                    Err(err) => {
                        tracing::error!("H.264 encoding failed: {}", err);
                        continue;
                    }
                }
            }

            tracing::info!("Frame receiver closed, encoder thread terminating");
            *done_clone.lock().unwrap() = true;
        });

        Ok(Self {
            buffer,
            done,
            _encoder_thread: Some(encoder_thread),
        })
    }
}

impl Read for H264Stream {
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

/// Convert a frame (assumed BGRA or RGBA) to YUV420 format for H.264 encoding
fn convert_to_yuv420(frame: &Frame) -> Result<YUVBuffer, VideoErrors> {
    let width = frame.width as usize;
    let height = frame.height as usize;
    let bytes_per_pixel = 4; // Assuming BGRA or RGBA

    // Ensure width and height are multiples of 2
    if width % 2 != 0 || height % 2 != 0 {
        return Err(VideoErrors::CannotCapture);
    }

    if frame.data.len() < width * height * bytes_per_pixel {
        return Err(VideoErrors::CannotCapture);
    }

    // Allocate YUV420 buffer (3/2 bytes per pixel: Y plane + U and V planes)
    let y_size = width * height;
    let uv_size = (width / 2) * (height / 2);
    let expected_size = y_size + 2 * uv_size;
    let mut yuv_data = vec![0u8; expected_size];

    let (y_plane, uv_planes) = yuv_data.split_at_mut(y_size);
    let (u_plane, v_plane) = uv_planes.split_at_mut(uv_size);

    // Convert BGRA to YUV420 using BT.601 color space conversion
    for y in 0..height {
        for x in 0..width {
            let pixel_idx = (y * width + x) * bytes_per_pixel;
            let b = frame.data[pixel_idx] as f32;
            let g = frame.data[pixel_idx + 1] as f32;
            let r = frame.data[pixel_idx + 2] as f32;
            // Alpha channel at pixel_idx + 3 is ignored

            // Y plane: standard BT.601 luma conversion
            let y_val = (0.299 * r + 0.587 * g + 0.114 * b) as u8;
            y_plane[y * width + x] = y_val;

            // U and V planes: sample only for every 2x2 block (chroma subsampling 4:2:0)
            if x % 2 == 0 && y % 2 == 0 {
                let u_val =
                    ((-0.168736 * r - 0.331264 * g + 0.5 * b + 128.0) as i32).clamp(0, 255) as u8;
                let v_val =
                    ((0.5 * r - 0.418688 * g - 0.081312 * b + 128.0) as i32).clamp(0, 255) as u8;

                let uv_idx = (y / 2) * (width / 2) + (x / 2);
                u_plane[uv_idx] = u_val;
                v_plane[uv_idx] = v_val;
            }
        }
    }

    // Create YUVBuffer from the prepared data
    // Safe because we've validated dimensions are multiples of 2 and buffer size is correct
    Ok(YUVBuffer::from_vec(yuv_data, width, height))
}
