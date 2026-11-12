use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("FinTS syntax error: {0}")]
    Syntax(String),
    #[error("Transport error: {0}")]
    Transport(String),
    #[error("Bank reported error {code}: {text}")]
    Bank { code: String, text: String },
    #[error("{0}")]
    Security(String),
    #[error("Cancelled by user")]
    Cancelled,
    #[error("Not supported by bank: {0}")]
    NotSupported(String),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
