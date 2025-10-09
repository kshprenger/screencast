use thiserror::Error;

#[derive(Error, Debug)]
pub enum TransportErrors {
    #[error("Connection failed")]
    ConnectionFailed,
    #[error("Connection is not openned")]
    ConnectionIsNotOpened,
    #[error("Failed tp send control message")]
    SendControlFailed,
    #[error("Failed to send data message")]
    SendDataFailed,
}
