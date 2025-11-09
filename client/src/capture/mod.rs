mod encoded_streams;
mod errors;
mod xcap;

pub use encoded_streams::H264Stream;
pub use errors::VideoErrors;
pub use xcap::XCapCapturer;

#[derive(Debug, Clone)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

pub trait ScreenCapturer {
    fn new() -> Result<Self, VideoErrors>
    where
        Self: Sized;
    fn start_capturing(&self) -> Result<std::sync::mpsc::Receiver<Frame>, VideoErrors>;
}
