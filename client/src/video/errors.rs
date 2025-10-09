use std::fmt;

#[derive(Debug)]
pub(super) enum VideoErrors {
    NoMonitorFound,
    CannotCapture,
}

impl fmt::Display for VideoErrors {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            VideoErrors::NoMonitorFound => write!(f, "No monitor found to record."),
            VideoErrors::CannotCapture => write!(f, "An error occurred during screen capturing."),
        }
    }
}
