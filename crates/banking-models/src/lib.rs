//! Data shared by the UI and native AqBanking adapter. No network transport,
//! FinTS message construction, bank-response parser or card cryptography lives here.
pub mod config;
pub mod display;
pub mod error;
pub mod interaction;
pub mod model;
pub mod payee;
pub mod validation;
pub use config::{AqBankingState, Config, State};
pub use error::{Error, Result};
pub use interaction::Interaction;
pub use model::*;
