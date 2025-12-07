use std::fmt::Display;

pub enum WebrtcEvents {
    GatheredAnswers,
    TrackArrived(tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>),
    TrackRemoved,
}

impl Display for WebrtcEvents {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WebrtcEvents::GatheredAnswers => f.write_str("GatheredAnswers"),
            WebrtcEvents::TrackArrived(_) => f.write_str("TrackArrived"),
            WebrtcEvents::TrackRemoved => f.write_str("TrackRemoved"),
        }
    }
}
