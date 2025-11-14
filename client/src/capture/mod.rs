mod errors;
mod scap;
mod xcap;

pub use errors::VideoErrors;
pub use scap::ScapCapturer;

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
