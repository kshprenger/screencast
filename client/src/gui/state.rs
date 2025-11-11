pub enum GUIState {
    Idle,
    Streaming,
    Watching(std::sync::mpsc::Receiver<crate::capture::Frame>),
}
