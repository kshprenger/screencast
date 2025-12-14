#[derive(thiserror::Error, Debug)]
pub enum CodecErrors {
    #[error("Cannot decode frame")]
    CannotDecode,
}
