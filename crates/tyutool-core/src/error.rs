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
    /// Anything a chip plugin failed at. Rendered bare: the payload is the
    /// message the user reads (CLI stderr, GUI banner, bridge `job_result`), and
    /// a "plugin error:" prefix in front of a sentence like "T5AI did not answer
    /// the download-mode handshake…" only names an implementation detail the
    /// user cannot act on. Classification never goes through this text —
    /// `is_device_no_response` and `auth_error_code` match the inner string.
    #[error("{0}")]
    Plugin(String),
}
