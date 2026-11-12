use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Account {
    pub iban: String,
    pub bic: String,
    pub account_number: String,
    pub sub_account: String,
    pub blz: String,
    pub owner: String,
    pub product: String,
    pub currency: String,
    /// UPD account type (1-9 current account, 10-19 savings, 50-59 credit card, ...)
    pub kind: u32,
    /// Business transactions the bank allows for this account (segment codes).
    pub allowed: Vec<String>,
}

impl Account {
    pub fn is_sepa(&self) -> bool {
        !self.iban.is_empty()
    }

    pub fn allows(&self, code: &str) -> bool {
        self.allowed.is_empty() || self.allowed.iter().any(|c| c == code)
    }
}

#[derive(Debug, Default)]
pub struct TransactionReport {
    pub transactions: Vec<Transaction>,
    /// Credit-card statement responses can also include the current booked balance.
    pub balance: Option<Balance>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Balance {
    pub booked: Decimal,
    pub available: Option<Decimal>,
    pub currency: String,
    pub date: NaiveDate,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Transaction {
    pub booking_date: NaiveDate,
    pub value_date: NaiveDate,
    /// The bank supplied neither booking nor value date. Dates above are only sort keys.
    #[serde(default)]
    pub date_unknown: bool,
    /// Positive = credit, negative = debit.
    pub amount: Decimal,
    pub currency: String,
    pub counterparty_name: String,
    pub counterparty_iban: String,
    pub counterparty_bic: String,
    pub purpose: String,
    pub posting_text: String,
    pub end_to_end_id: String,
    pub mandate_id: String,
    pub creditor_id: String,
    pub pending: bool,
    /// Stable hash used for de-duplication across repeated fetches.
    pub fingerprint: String,
}

impl Transaction {
    pub fn compute_fingerprint(&mut self) {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        if !self.date_unknown {
            self.booking_date.hash(&mut h);
            self.value_date.hash(&mut h);
        }
        self.amount.to_string().hash(&mut h);
        self.counterparty_iban.hash(&mut h);
        self.purpose.hash(&mut h);
        self.end_to_end_id.hash(&mut h);
        self.fingerprint = format!("{:016x}", h.finish());
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SepaTransfer {
    pub debtor_name: String,
    pub debtor_iban: String,
    pub debtor_bic: String,
    pub creditor_name: String,
    pub creditor_iban: String,
    pub creditor_bic: String,
    pub amount: Decimal,
    pub purpose: String,
    pub end_to_end_id: String,
    /// Requested execution date; `None` = as soon as possible.
    pub execution_date: Option<NaiveDate>,
    pub instant: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Interval {
    Monthly,
    Weekly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StandingOrder {
    pub order_id: String,
    pub transfer: SepaTransfer,
    pub first_date: NaiveDate,
    pub last_date: Option<NaiveDate>,
    #[serde(default)]
    pub execution_time: Option<chrono::NaiveTime>,
    pub interval: Interval,
    /// Every n months/weeks.
    pub every: u32,
    /// Day of month (1-31, 97=ultimo-2, 98=ultimo-1, 99=ultimo) or weekday (1-7).
    pub day: u32,
    /// Raw pain document as returned by the bank (needed for deletion).
    #[serde(default)]
    pub raw_pain: Vec<u8>,
    #[serde(default)]
    pub descriptor: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TanMethod {
    pub security_function: String,
    pub name: String,
    pub technical_id: String,
    pub dk_id: String,
    pub tan_process: String,
    pub decoupled: bool,
    pub needs_medium: bool,
    pub max_tan_length: u32,
    pub hhduc_response_required: bool,
    pub decoupled_poll_wait_first: u32,
    pub decoupled_poll_wait_next: u32,
}

impl TanMethod {
    /// Rough classification for the UI.
    pub fn kind(&self) -> TanKind {
        let id = format!("{} {} {}", self.dk_id, self.technical_id, self.name).to_lowercase();
        if self.decoupled || id.contains("decoupled") {
            TanKind::Decoupled
        } else if id.contains("hhd") || id.contains("chiptan") || id.contains("smarttan") {
            TanKind::ChipTan
        } else if id.contains("qr") || id.contains("photo") || id.contains("matrix") {
            TanKind::Matrix
        } else if id.contains("sms") || id.contains("mobile") {
            TanKind::Sms
        } else {
            TanKind::Other
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TanKind {
    ChipTan,
    Matrix,
    Decoupled,
    Sms,
    Other,
}

#[derive(Debug, Clone)]
pub struct TanChallenge {
    pub text: String,
    /// Raw HHDuc payload (flicker code start for chipTAN, image for photoTAN/QR).
    pub hhduc: Option<Vec<u8>>,
    pub method: TanMethod,
    pub decoupled: bool,
    pub task_reference: String,
}

#[derive(Debug, Clone)]
pub enum TanAnswer {
    Tan(String),
    /// User confirmed in the banking app (decoupled); client polls status.
    Confirmed,
    Cancel,
}
