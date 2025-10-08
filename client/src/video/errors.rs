use std::fmt;

#[derive(Debug)]
pub(super) enum VideoErrors {
    NoMonitorError,
    RecordError,
}

impl fmt::Display for VideoErrors {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            VideoErrors::NoMonitorError => write!(f, "No primary monitor found to record."),
            VideoErrors::RecordError => write!(f, "An error occurred during video recording."),
        }
    }
}
