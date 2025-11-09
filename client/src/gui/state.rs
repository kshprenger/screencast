pub(super) enum GUIState {
    Idle,
    Streaming,
    Watching(tokio::sync::mpsc::Receiver<crate::capture::Frame>),
}
