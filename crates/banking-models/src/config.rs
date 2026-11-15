use serde::{Deserialize, Serialize};
use crate::Account;

/// Static access configuration of one bank login.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    pub blz: String,
    pub user_id: String,
    /// Kunden-ID; defaults to the user id when empty.
    pub customer_id: String,
    /// https://... for PIN/TAN, host[:port] for TCP (chip card).
    pub url: String,
    /// FinTS product registration number (Deutsche Kreditwirtschaft).
    pub product_id: String,
    pub product_version: String,
}
/// Native settings only. Serde ignores obsolete fields in old vaults.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub aqbanking: Option<AqBankingState>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct AqBankingState {
    pub version: u32,
    pub binding: String,
    pub snapshot: Vec<u8>,
    pub accounts: Vec<(u32, Account)>,
}
impl std::fmt::Debug for AqBankingState {
    fn fmt(&self,f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AqBankingState").field("version",&self.version).finish_non_exhaustive()
    }
}
impl Drop for AqBankingState {
    fn drop(&mut self) { zeroize::Zeroize::zeroize(&mut self.snapshot); }
}


#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_vault_state_is_read_without_preserving_the_removed_backend() {
        let old=serde_json::json!({"system_id":"old system","bpd":{"versions":{"HKSAL":[5]}},
            "upd":{"accounts":[]},"bank_keys":{"old":"public keys"},"tan_method":"999","allowed_methods":["999"]});
        let state:State=serde_json::from_value(old).unwrap();
        assert!(state.aqbanking.is_none());
        assert_eq!(serde_json::to_value(state).unwrap(),serde_json::json!({"aqbanking":null}));
        let mut config=serde_json::to_value(Config{url:"https://bank.invalid".into(),..Default::default()}).unwrap();
        config["statement_format"]=serde_json::json!("Mt940");
        assert_eq!(serde_json::from_value::<Config>(config).unwrap().url,"https://bank.invalid");
    }
    #[test]
    fn native_snapshot_and_account_ids_survive_vault_serialization() {
        let state=State{aqbanking:Some(AqBankingState{version:1,binding:"test".into(),snapshot:vec![0,255,12],
            accounts:vec![(17,Account{iban:"DE02120300000000202051".into(),..Default::default()})]})};
        let roundtrip:State=serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
        let native=roundtrip.aqbanking.as_ref().unwrap();
        assert_eq!(native.snapshot,[0,255,12]);assert_eq!(native.accounts[0].0,17);assert_eq!(native.binding,"test");
    }
}
