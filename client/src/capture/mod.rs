mod errors;
mod scap;

pub use errors::VideoErrors;
pub use scap::ScapCapturer;
use tokio_util::sync::CancellationToken;

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
    fn start_capturing(
        &self,
        ctx: CancellationToken,
    ) -> Result<std::sync::mpsc::Receiver<Frame>, VideoErrors>;
}
