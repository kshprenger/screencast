#[derive(thiserror::Error, Debug)]
pub enum VideoErrors {
    #[error("No monitor found to record")]
    NoMonitorFound,
}
