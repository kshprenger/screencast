#[derive(Debug)]
pub enum WebrtcEvents {
    GatheredAnswers,
    TrackArrived(tokio::sync::mpsc::Receiver<crate::capture::Frame>),
}
