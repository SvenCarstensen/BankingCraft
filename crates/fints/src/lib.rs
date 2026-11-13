//! FinTS 3.0 client library used by BankingCraft.
//!
//! * PIN/TAN with PSD2 strong customer authentication (HKTAN v6/v7, process 4/2/S,
//!   decoupled methods, chipTAN optical/USB, photoTAN/QR)
//! * HBCI chip card (DDV) via the [`security::DdvCard`] trait
//! * RDH chip card (RDH-7/9/10, e.g. VR-NetWorld-Card) via the [`rdh::RdhCard`] trait
//! * Business transactions: balances, statements (MT940/camt.052), SEPA transfers,
//!   instant and scheduled transfers, standing orders

pub mod bpd;
pub mod bank_profile;
pub mod camt;
pub mod client;
mod credit_card;
pub mod error;
pub mod model;
pub mod mt940;
pub mod pain;
pub mod rdh;
#[cfg(test)]
mod rdh_flow_test;
pub mod response;
pub mod security;
pub mod syntax;
pub mod tan;
pub mod transport;
pub mod vop;
#[cfg(test)]
mod business_data_tests;
#[cfg(test)]
mod security_tests;

pub use client::{Client, Config, Interaction, Medium, State};
pub use error::{Error, Result};
pub use model::*;

#[cfg(test)]
mod regression_tests;
