//! FinTS 3.0 client library used by Kontor.
//!
//! * PIN/TAN with PSD2 strong customer authentication (HKTAN v6/v7, process 4/2/S,
//!   decoupled methods, chipTAN optical/USB, photoTAN/QR)
//! * HBCI chip card (DDV) via the [`security::DdvCard`] trait
//! * Business transactions: balances, statements (MT940/camt.052), SEPA transfers,
//!   instant and scheduled transfers, standing orders

pub mod bpd;
pub mod camt;
pub mod client;
pub mod error;
pub mod model;
pub mod mt940;
pub mod pain;
pub mod response;
pub mod security;
pub mod syntax;
pub mod tan;
pub mod transport;

pub use client::{Client, Config, Interaction, Medium, State};
pub use error::{Error, Result};
pub use model::*;
