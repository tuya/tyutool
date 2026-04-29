use thiserror::Error;

#[derive(Debug, Error)]
pub enum FlashError {
    #[error("unknown chip plugin: {0}")]
    UnknownChip(String),
    #[error("serial: {0}")]
    Serial(#[from] serialport::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("operation cancelled")]
    Cancelled,
    #[error("invalid job: {0}")]
    InvalidJob(String),
    #[error("plugin error: {0}")]
    Plugin(String),
}
