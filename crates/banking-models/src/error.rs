#[derive(Debug)]
pub enum Error {
    Transport(String),
    Bank { code: String, text: String },
    /// Failure preparing the bank access, before an account job can run.
    Access { code: String, text: String },
    Security(String),
    Cancelled,
    NotSupported(String),
    Other(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::Transport(text) => i18n::tr_format!("Übertragungsfehler: {0}", text),
            Self::Bank { code, text } => i18n::tr_format!("Die Bank meldet Fehler {code}: {text}"),
            Self::Access { text, .. } => format!("Bankverbindung konnte nicht vorbereitet werden:\n{text}"),
            Self::Security(text) | Self::Other(text) => return f.write_str(text),
            Self::Cancelled => i18n::tr("Vom Benutzer abgebrochen").to_string(),
            Self::NotSupported(text) => i18n::tr_format!("Von der Bank nicht unterstützt: {0}", text),
        };
        f.write_str(&message)
    }
}
impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
