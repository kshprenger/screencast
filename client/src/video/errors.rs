#[derive(thiserror::Error, Debug)]
pub enum VideoErrors {
    #[error("No monitor found to record")]
    NoMonitorFound,
    #[error("An error occurred during screen capturing")]
    CannotCapture,
}
