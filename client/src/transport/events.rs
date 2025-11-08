pub enum WebrtcEvents {
    GatheredAnswers,
    TrackArrived(tokio::sync::mpsc::Receiver<crate::video::Frame>),
}
