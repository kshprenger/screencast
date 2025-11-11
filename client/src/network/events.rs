use std::fmt::Display;

pub enum WebrtcEvents {
    GatheredAnswers,
    TrackArrived(std::sync::mpsc::Receiver<crate::capture::Frame>),
}

impl Display for WebrtcEvents {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WebrtcEvents::GatheredAnswers => f.write_str("GatheredAnswers"),
            WebrtcEvents::TrackArrived(_) => f.write_str("TrackArrived"),
        }
    }
}
