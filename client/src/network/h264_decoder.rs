use ffmpeg_next::format::Pixel;
use ffmpeg_next::software::scaling::{context::Context, flag::Flags};
use ffmpeg_next::{codec, frame};
use std::collections::VecDeque;

use crate::capture::Frame;

/// H264 decoder that converts H264 encoded data to RGBA frames
pub struct H264Decoder {
    decoder: codec::decoder::Video,
    scaler: Option<Context>,
    nal_buffer: VecDeque<u8>,
}

impl H264Decoder {
    pub fn new() -> Self {
        let _ = ffmpeg_next::init();

        let codec = codec::decoder::find(codec::Id::H264).expect("Could not find H264 decoder");
        let decoder = codec::context::Context::new_with_codec(codec)
            .decoder()
            .video()
            .expect("Could not create video decoder context");

        Self {
            decoder,
            scaler: None,
            nal_buffer: VecDeque::new(),
        }
    }

    /// Decode an RTP packet payload containing H264 data
    /// Returns Ok(Some(frame)) if a complete frame was decoded, Ok(None) if more data needed
    pub fn decode_rtp_packet(&mut self, payload: &[u8]) -> Result<Option<Frame>, String> {
        // Add payload to buffer
        self.nal_buffer.extend(payload.iter().copied());

        // Try to decode NAL units from buffer
        self.decode_buffer()
    }

    fn decode_buffer(&mut self) -> Result<Option<Frame>, String> {
        let buffer_bytes: Vec<u8> = self.nal_buffer.iter().copied().collect();

        if buffer_bytes.is_empty() {
            return Ok(None);
        }

        let input_packet = ffmpeg_next::packet::Packet::copy(&buffer_bytes);

        self.decoder
            .send_packet(&input_packet)
            .map_err(|e| format!("Failed to send packet to decoder: {e}"))?;

        // Try to receive decoded frame
        let mut decoded_frame = frame::Video::empty();
        match self.decoder.receive_frame(&mut decoded_frame) {
            Ok(()) => {
                // Clear the buffer since we've consumed it
                self.nal_buffer.clear();

                // Convert YUV to RGBA
                let rgba_frame = self.convert_to_rgba(&decoded_frame)?;
                Ok(Some(rgba_frame))
            }
            Err(e) => {
                tracing::error!("Decoder needs more data: {e}");
                // Decoder needs more data
                Ok(None)
            }
        }
    }

    fn convert_to_rgba(&mut self, frame: &frame::Video) -> Result<Frame, String> {
        let width = frame.width();
        let height = frame.height();
        let input_format = frame.format();

        // Initialize or update scaler if needed
        if self.scaler.is_none() {
            self.scaler = Context::get(
                input_format,
                width,
                height,
                Pixel::RGBA,
                1080, // ???
                720,
                Flags::BILINEAR,
            )
            .ok();
        }

        let mut rgba_frame = frame::Video::new(Pixel::RGBA, width, height);

        self.scaler
            .as_mut()
            .ok_or_else(|| "Scaler not initialized".to_string())?
            .run(frame, &mut rgba_frame)
            .map_err(|e| format!("Failed to scale frame: {e}"))?;

        // Extract RGBA data
        let rgba_data = rgba_frame.data(0).to_vec();

        Ok(Frame {
            width,
            height,
            data: rgba_data,
        })
    }
}

impl Default for H264Decoder {
    fn default() -> Self {
        Self::new()
    }
}
