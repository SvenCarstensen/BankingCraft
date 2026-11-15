//! Presentation models for the offline demo and UI. No bank-protocol parser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PayeeStatus { Match, CloseMatch, NoMatch, Unavailable }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayeeVerification {
    pub status: PayeeStatus,
    pub name: String,
    pub iban: String,
    pub suggested_name: String,
    pub reason: String,
    pub bank_notice: String,
}
