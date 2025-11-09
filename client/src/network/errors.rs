#[derive(thiserror::Error, Debug)]
pub enum NetworkErrors {
    #[error("Connection failed")]
    ConnectionFailed,
    #[error("Connection is not openned")]
    ConnectionIsNotOpened,
    #[error("Failed tp send control message")]
    SendControlFailed,
    #[error("Failed to send data message")]
    SendDataFailed,
    #[error("Failed to (de)serialize message")]
    SerdeError(#[from] serde_json::Error),
}
