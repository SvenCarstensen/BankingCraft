//! One-time import of account settings transcribed from Bank X's account settings.
//! No PINs, private keys, balances or bank security state are accepted.
use crate::store::{AccessKind, BankAccess, Vault, VaultFile};
use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Import {
    version: u32,
    accesses: Vec<Access>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Access {
    name: String,
    blz: String,
    user_id: String,
    customer_id: String,
    host: String,
    accounts: Vec<Account>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Account {
    iban: String,
    bic: String,
    number: String,
    sub_account: String,
    owner: String,
    name: String,
    currency: String,
}

fn merge(vault: &Vault, bytes: &[u8]) -> Result<(Vault, usize, usize), String> {
    let import: Import = serde_json::from_slice(bytes).map_err(|_| i18n::tr("Ungültige Bank-X-Importdatei"))?;
    if import.version != 1 || import.accesses.is_empty() || import.accesses.len() > 100 {
        return Err(i18n::tr("Nicht unterstütztes Bank-X-Importformat").into());
    }
    let mut candidate = vault.clone();
    let mut accesses = 0;
    let mut accounts = 0;
    for a in import.accesses {
        if a.blz.len() != 8 || !a.blz.bytes().all(|c| c.is_ascii_digit())
            || a.user_id.trim().is_empty() || a.name.trim().is_empty()
            || a.host.is_empty() || !a.host.bytes().all(|c| c.is_ascii_alphanumeric() || b".-".contains(&c))
            || a.accounts.is_empty() || a.accounts.len() > 1000
        {
            return Err(i18n::tr("Unvollständiger RDH-Bankzugang in der Importdatei").into());
        }
        let existing = candidate.accesses.iter().find(|old| {
            matches!(old.kind, AccessKind::RdhCard { .. })
                && old.config.blz == a.blz && old.config.user_id == a.user_id
        });
        let access_id = if let Some(old) = existing {
            // Keep existing reader selection, verified bank keys and user settings.
            old.id
        } else {
            let id = candidate.new_id();
            candidate.accesses.push(BankAccess {
                id, name: a.name,
                kind: AccessKind::RdhCard { reader: String::new(), record: 0 },
                config: banking::Config {
                    blz: a.blz.clone(), user_id: a.user_id, customer_id: a.customer_id,
                    url: a.host, ..Default::default()
                },
                state: Default::default(), saved_pin: None,
            });
            accesses += 1;
            id
        };
        for account in a.accounts {
            let iban = account.iban.replace(' ', "").to_ascii_uppercase();
            if !banking::validation::iban_valid(&iban) || account.number.is_empty()
                || !account.number.bytes().all(|c| c.is_ascii_digit())
                || account.currency.len() != 3 || !account.currency.bytes().all(|c| c.is_ascii_uppercase())
                || (iban.starts_with("DE") && (iban.len() != 22 || iban[4..12] != a.blz
                    || iban[12..].trim_start_matches('0') != account.number.trim_start_matches('0')))
            {
                return Err(i18n::tr("Ungültige oder widersprüchliche Kontoverbindung in der Importdatei").into());
            }
            if candidate.accounts.iter().any(|old| old.access_id == access_id
                && (old.account.iban == iban || (old.account.account_number.trim_start_matches('0') == account.number.trim_start_matches('0')
                    && old.account.sub_account == account.sub_account))) {
                continue;
            }
            candidate.merge_accounts(access_id, vec![banking::Account {
                iban, bic: account.bic, account_number: account.number, sub_account: account.sub_account,
                blz: a.blz.clone(), owner: account.owner, product: account.name, currency: account.currency,
                // Actual capabilities and account types are refreshed from the bank's UPD.
                allowed: vec!["HKSAL".into(), "HKKAZ".into(), "HKCAZ".into()],
                ..Default::default()
            }]);
            accounts += 1;
        }
    }
    Ok((candidate, accesses, accounts))
}

/// Commit only after a successful vault unlock and encrypted save. Retry is idempotent.
pub fn pending(vault: &mut Vault, file: &VaultFile) -> Result<Option<String>, String> {
    let path = file.path().with_file_name("bankx-import.json");
    if !path.exists() { return Ok(None); }
    import_file(vault, file, &path).map(Some)
}

fn import_file(vault: &mut Vault, file: &VaultFile, path: &Path) -> Result<String, String> {
    use std::io::{Read, Write};
    let input = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = zeroize::Zeroizing::new(Vec::new());
    input.take(1_048_577).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() > 1_048_576 { return Err(i18n::tr("Bank-X-Importdatei ist zu groß").into()); }
    let (candidate, accesses, accounts) = merge(vault, &bytes)?;
    let backup = file.path().with_extension("before-bankx-import");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)] {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(backup) {
        Ok(mut backup) => {
            backup.write_all(&std::fs::read(file.path()).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            backup.sync_all().map_err(|e| e.to_string())?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {},
        Err(e) => return Err(i18n::tr_format!("Sicherung vor Import fehlgeschlagen: {e}")),
    }
    file.save(&candidate)?;
    *vault = candidate;
    let mut message = i18n::tr_format!("Bank X: {accesses} Bankzugänge und {accounts} Konten übernommen. Für aktuelle Salden bitte den Zugang abrufen.");
    if std::fs::remove_file(path).is_err() {
        message.push_str(i18n::tr(" Die temporäre Importdatei konnte nicht entfernt werden."));
    }
    log::info!("Bank-X-Import gespeichert: {accesses} Zugänge, {accounts} Konten");
    Ok(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<u8> {
        br#"{"version":1,"accesses":[{"name":"Testbank","blz":"37040044","user_id":"test","customer_id":"test","host":"bank.example","accounts":[{"iban":"DE89370400440532013000","bic":"COBADEFFXXX","number":"532013000","sub_account":"","owner":"Test","name":"Testkonto","currency":"EUR"}]}]}"#.to_vec()
    }
    #[test]
    fn import_rejects_unknown_fields_versions_empty_accesses_and_invalid_currency() {
        for (from, to) in [("\"version\":1", "\"version\":2"), ("\"version\":1", "\"version\":1,\"pin\":\"not-allowed\""), ("EUR", "eur"), ("37040044", "3704x044"), ("bank.example", "https://bank.example"), ("532013000", "532013001")] {
            let input = String::from_utf8(fixture()).unwrap().replace(from, to);
            assert!(merge(&Vault::default(), input.as_bytes()).is_err(), "{from}");
        }
        assert!(merge(&Vault::default(), br#"{"version":1,"accesses":[]}"#).is_err());
        assert!(merge(&Vault::default(), b"not json").is_err());
    }

    #[test]
    fn import_normalizes_iban_and_preserves_config_groups_and_account_order_on_reimport() {
        let input = String::from_utf8(fixture()).unwrap().replace("DE89370400440532013000", "de89 3704 0044 0532 0130 00");
        let (mut vault, _, _) = merge(&Vault::default(), input.as_bytes()).unwrap();
        assert_eq!(vault.accounts[0].account.iban, "DE89370400440532013000");
        vault.accesses[0].kind = AccessKind::RdhCard { reader: "My Reader".into(), record: 2 };
        let group = vault.create_group("VR Bank").unwrap(); vault.assign_group(vault.accounts[0].id, Some(group)).unwrap();
        let original = serde_json::to_vec(&vault).unwrap(); let (updated, a, c) = merge(&vault, &fixture()).unwrap();
        assert_eq!((a, c), (0, 0)); assert_eq!(serde_json::to_vec(&updated).unwrap(), original);
    }

    #[test]
    fn malformed_staging_file_leaves_vault_and_input_untouched() {
        let dir = std::env::temp_dir().join(format!("bankingcraft-bad-import-{:016x}", rand::random::<u64>())); std::fs::create_dir(&dir).unwrap();
        let path = dir.join("test.vault"); let file = VaultFile::create(path.clone(), "synthetic-test-password").unwrap(); let mut vault = Vault::default(); file.save(&vault).unwrap();
        let original = std::fs::read(&path).unwrap(); let input = dir.join("bankx-import.json");
        for data in [b"bad json".to_vec(), vec![b' '; 1_048_577]] {
            std::fs::write(&input, &data).unwrap(); assert!(import_file(&mut vault, &file, &input).is_err());
            assert_eq!(std::fs::read(&input).unwrap(), data); assert_eq!(std::fs::read(&path).unwrap(), original); assert!(vault.accesses.is_empty());
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn import_is_idempotent_and_preserves_existing_data() {
        let (mut first, a, c) = merge(&Vault::default(), &fixture()).unwrap();
        assert_eq!((a,c), (1,1));
        first.accesses[0].state.aqbanking.get_or_insert_with(Default::default).binding = "keep".into();
        first.accounts[0].account.product = "Mein Konto".into();
        let (second, a, c) = merge(&first, &fixture()).unwrap();
        assert_eq!((a,c), (0,0));
        assert_eq!(second.accesses[0].state.aqbanking.as_ref().unwrap().binding, "keep");
        assert_eq!(second.accounts[0].account.product, "Mein Konto");
    }
    #[test]
    fn bad_account_or_host_cannot_partially_import() {
        let original = Vault::default();
        let bad = String::from_utf8(fixture()).unwrap().replace("532013000", "532013001");
        assert!(merge(&original, bad.as_bytes()).is_err());
        let bad = String::from_utf8(fixture()).unwrap().replace("bank.example", "bank.example/redirect");
        assert!(merge(&original, bad.as_bytes()).is_err());
        assert!(original.accesses.is_empty());
    }
    #[test]
    fn imported_accounts_are_saved_encrypted_before_staging_file_is_removed() {
        let dir = std::env::temp_dir().join(format!("bankingcraft-import-{:016x}", rand::random::<u64>()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("vault.kontor");
        let file = VaultFile::create(path.clone(), "only-a-test-password").unwrap();
        let mut vault = Vault::default();
        file.save(&vault).unwrap();
        let pending = dir.join("bankx-import.json");
        std::fs::write(&pending, fixture()).unwrap();
        import_file(&mut vault, &file, &pending).unwrap();
        assert!(!pending.exists());
        assert_eq!(VaultFile::open(path.clone(), "only-a-test-password").unwrap().1.accounts.len(), 1);
        assert!(VaultFile::open(path.with_extension("before-bankx-import"), "only-a-test-password").unwrap().1.accounts.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
