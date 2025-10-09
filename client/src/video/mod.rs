mod errors;
mod xcap;

use errors::VideoErrors;
use tokio::sync::mpsc;

// 10 seconds in 60 fps
const FRAME_BUFFER_SIZE: usize = 10 * 60;

#[derive(Debug)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

#[async_trait::async_trait]
pub trait ScreenCapturer {
    fn new() -> Result<Self, VideoErrors>
    where
        Self: Sized;
    async fn start_capturing(&self) -> Result<mpsc::Receiver<Frame>, VideoErrors>;
}
