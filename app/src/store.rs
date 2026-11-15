//! Encrypted local vault: AES-256-GCM, key derived from the master password with Argon2id.
//! File layout: magic (8) | salt (16) | nonce (12) | ciphertext.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use chrono::NaiveDateTime;
use banking::{Account, Balance, StandingOrder, Transaction};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

// Keep the existing vault format and secret derivation compatible after the BankingCraft rename.
const MAGIC: &[u8; 8] = b"KONTOR1\0";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AccessKind {
    PinTan,
    ChipCard { reader: String, record: u8 },
    /// RDH chip card (ZKA/SECCOS); `record` = 1-based entry, or 0 to match bank/user automatically.
    RdhCard { reader: String, record: u8 },
    /// Read-only PayPal NVP API. API credentials are sealed separately from vault data.
    PayPal { sandbox: bool, credentials: SealedSecret },
    Demo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BankAccess {
    pub id: u32,
    pub name: String,
    pub kind: AccessKind,
    pub config: banking::Config,
    pub state: banking::State,
    /// PIN saved on request – encrypted separately (see [`VaultFile::seal_secret`]).
    #[serde(default)]
    pub saved_pin: Option<SealedSecret>,
}

/// A secret (PIN) encrypted with AES-256-GCM under a key derived from the vault key.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SealedSecret {
    pub nonce: Vec<u8>,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredAccount {
    pub id: u32,
    pub access_id: u32,
    pub account: Account,
    /// Local display name; bank refreshes only replace `account`.
    #[serde(default)]
    pub display_name: String,
    pub balance: Option<Balance>,
    pub last_sync: Option<NaiveDateTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredTx {
    pub account_id: u32,
    pub tx: Transaction,
    /// Manually assigned category.
    pub category: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredStanding {
    pub account_id: u32,
    pub order: StandingOrder,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub pattern: String,
    pub category: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountGroup {
    pub id: u32,
    pub name: String,
    pub account_ids: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Vault {
    pub next_id: u32,
    pub accesses: Vec<BankAccess>,
    pub accounts: Vec<StoredAccount>,
    pub transactions: Vec<StoredTx>,
    pub standing: Vec<StoredStanding>,
    pub rules: Vec<Rule>,
    /// User-defined category names, stored inside the encrypted vault.
    #[serde(default)]
    pub custom_categories: Vec<String>,
    /// Stable built-in category identities with vault-specific labels or replacements.
    #[serde(default)]
    pub category_overrides: std::collections::BTreeMap<String, crate::categories::CategoryOverride>,
    pub product_id: String,
    pub product_version: String,
    pub chiptan_reader: String,
    #[serde(default)]
    pub groups: Vec<AccountGroup>,
    #[serde(default)]
    pub payment_orders: Vec<crate::payments::PaymentOrder>,
    #[serde(default)]
    pub recipients: Vec<crate::payments::Recipient>,
}

impl Vault {
    pub fn new_id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }

    pub fn account(&self, id: u32) -> Option<&StoredAccount> {
        self.accounts.iter().find(|a| a.id == id)
    }

    pub fn rename_account(&mut self, id: u32, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.chars().any(char::is_control) {
            return Err(i18n::tr("Bitte einen Anzeigenamen ohne Zeilenumbrüche oder Steuerzeichen eingeben.").into());
        }
        self.accounts.iter_mut().find(|a| a.id == id).ok_or(i18n::tr("Konto nicht gefunden"))?.display_name = name.into();
        Ok(())
    }

    pub fn access(&self, id: u32) -> Option<&BankAccess> {
        self.accesses.iter().find(|a| a.id == id)
    }

    pub fn remove_access(&mut self, id: u32) {
        let account_ids: Vec<u32> = self.accounts.iter().filter(|a| a.access_id == id).map(|a| a.id).collect();
        self.accesses.retain(|a| a.id != id);
        self.accounts.retain(|a| a.access_id != id);
        self.transactions.retain(|t| !account_ids.contains(&t.account_id));
        self.standing.retain(|s| !account_ids.contains(&s.account_id));
        self.payment_orders.retain(|o| !account_ids.contains(&o.account_id));
        self.recipients.retain(|r| !account_ids.contains(&r.account_id));
        for group in &mut self.groups {
            group.account_ids.retain(|id| !account_ids.contains(id));
        }
    }

    /// Insert or update accounts of an access, returning their ids in the given order.
    pub fn merge_accounts(&mut self, access_id: u32, accounts: Vec<Account>) -> Vec<u32> {
        let mut ids = Vec::new();
        for acc in accounts {
            let existing = self.accounts.iter_mut().find(|s| {
                s.access_id == access_id
                    && ((!acc.iban.is_empty() && s.account.iban == acc.iban)
                        || (!acc.account_number.is_empty() && s.account.account_number == acc.account_number
                            && s.account.sub_account == acc.sub_account && s.account.blz == acc.blz))
            });
            match existing {
                Some(s) => {
                    s.account = acc;
                    ids.push(s.id);
                }
                None => {
                    let id = self.new_id();
                    self.accounts.push(StoredAccount { id, access_id, account: acc, display_name: String::new(), balance: None, last_sync: None });
                    ids.push(id);
                }
            }
        }
        ids
    }

    /// Merge fetched transactions: booked ones are de-duplicated by fingerprint,
    /// pending ones replace the previous pending set of that account.
    pub fn merge_transactions(&mut self, account_id: u32, txs: Vec<Transaction>) -> usize {
        self.merge_transactions_by(account_id,txs,|tx|tx.fingerprint.clone())
    }

    /// Different statement parsers represent decimal scale and line wrapping
    /// differently. Match exact normalized fields with multiplicity; preserve
    /// stored text/categories and never use approximate amount/date matching.
    pub fn merge_aqbanking_transactions(&mut self, account_id:u32, txs:Vec<Transaction>) -> usize {
        self.merge_transactions_by(account_id,txs,|tx| {
            let mut normalized=tx.clone();
            normalized.amount=normalized.amount.normalize();
            normalized.purpose=normalized.purpose.split_whitespace().collect::<Vec<_>>().join(" ");
            normalized.counterparty_iban=normalized.counterparty_iban.split_whitespace().collect::<String>().to_uppercase();
            normalized.end_to_end_id=normalized.end_to_end_id.trim().to_owned();
            if normalized.end_to_end_id.eq_ignore_ascii_case("NOTPROVIDED") { normalized.end_to_end_id.clear(); }
            normalized.compute_fingerprint();
            format!("{}:{}:{}",normalized.fingerprint,normalized.currency.to_uppercase(),normalized.date_unknown)
        })
    }

    fn merge_transactions_by(&mut self, account_id:u32, txs:Vec<Transaction>, key:impl Fn(&Transaction)->String) -> usize {
        self.transactions.retain(|t| !(t.account_id == account_id && t.tx.pending));
        let mut added = 0;
        let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for t in self.transactions.iter().filter(|t| t.account_id == account_id) {
            *seen.entry(key(&t.tx)).or_default() += 1;
        }
        // identical bookings on the same day are legitimate: compare counts per batch
        let mut batch: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for tx in txs {
            if tx.pending {
                self.transactions.push(StoredTx { account_id, tx, category: None });
                continue;
            }
            let fingerprint=key(&tx);
            let n = batch.entry(fingerprint.clone()).or_default();
            *n += 1;
            if seen.get(&fingerprint).copied().unwrap_or(0) < *n {
                added += 1;
                self.transactions.push(StoredTx { account_id, tx, category: None });
            }
        }
        added
    }

    pub fn last_booking(&self, account_id: u32) -> Option<chrono::NaiveDate> {
        self.transactions
            .iter()
            .filter(|t| t.account_id == account_id && !t.tx.pending && !t.tx.date_unknown)
            .map(|t| t.tx.booking_date)
            .max()
    }
}

pub struct VaultFile {
    path: PathBuf,
    salt: [u8; 16],
    key: Zeroizing<[u8; 32]>,
    // A separate inode survives atomic replacement of the encrypted vault.
    session_lock: Option<std::sync::Arc<std::fs::File>>,
}

fn derive(password: &str, salt: &[u8; 16]) -> Result<Zeroizing<[u8; 32]>, String> {
    let mut key = Zeroizing::new([0u8; 32]);
    argon2::Argon2::default()
        .hash_password_into(password.as_bytes(), salt, key.as_mut())
        .map_err(|e| e.to_string())?;
    Ok(key)
}

pub fn default_path() -> PathBuf {
    // Retain the established storage location so existing vaults remain discoverable.
    directories::ProjectDirs::from("de", "AppCreation", "Kontor")
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
        .join("vault.kontor")
}

impl VaultFile {
    pub fn exists(path: &Path) -> bool {
        path.exists()
    }

    pub fn create(path: PathBuf, password: &str) -> Result<Self, String> {
        let mut salt = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut salt);
        let key = derive(password, &salt)?;
        Ok(Self { path, salt, key, session_lock: None })
    }

    /// Acquire the session lock before reading: another process may otherwise
    /// replace the vault between decrypting it and acquiring ownership.
    pub fn open_session(path: PathBuf, password: &str) -> Result<(Self, Vault), String> {
        let path = path.canonicalize().map_err(|e| i18n::tr_format!("Tresor nicht lesbar: {e}"))?;
        let guard = Self::lock_session(&path)?;
        let (mut file, vault) = Self::open(path, password)?;
        file.session_lock = Some(guard);
        Ok((file, vault))
    }

    pub fn reopen_session(&self, password: &str) -> Result<(Self, Vault), String> {
        let (mut file, vault) = Self::open(self.path.clone(), password)?;
        file.session_lock = self.session_lock.clone();
        Ok((file, vault))
    }

    fn lock_session(path: &Path) -> Result<std::sync::Arc<std::fs::File>, String> {
        let parent = path.parent().ok_or("Tresorpfad ohne Verzeichnis")?;
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let parent = parent.canonicalize().map_err(|e| e.to_string())?;
        let mut name = path.file_name().ok_or("Ungültiger Tresorpfad")?.to_os_string();
        name.push(".lock");
        let lock_path = parent.join(name);
        if std::fs::symlink_metadata(&lock_path).is_ok_and(|m| !m.is_file()) {
            return Err("Ungültige Tresorsperrdatei".into());
        }
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)] {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        #[cfg(windows)] {
            use std::os::windows::fs::OpenOptionsExt;
            options.custom_flags(0x00200000); // FILE_FLAG_OPEN_REPARSE_POINT
        }
        let guard = options.open(lock_path).map_err(|e| e.to_string())?;
        let metadata=guard.metadata().map_err(|e|e.to_string())?;
        if !metadata.is_file() { return Err("Ungültige Tresorsperrdatei".into()); }
        #[cfg(unix)] {
            use std::os::unix::fs::MetadataExt;
            if metadata.nlink()!=1 { return Err("Verknüpfte Tresorsperrdatei wird nicht verwendet".into()); }
        }
        #[cfg(windows)] {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 { return Err("Verknüpfte Tresorsperrdatei wird nicht verwendet".into()); }
        }
        guard.try_lock().map_err(|_| "Dieser Tresor wird bereits von einer anderen BankingCraft-Instanz verwendet.".to_string())?;
        // Do not unlink lock files: removing a locked inode permits a second
        // process to acquire a new inode for the same vault.
        Ok(std::sync::Arc::new(guard))
    }

    pub fn open(path: PathBuf, password: &str) -> Result<(Self, Vault), String> {
        use std::io::Read;
        let path = path.canonicalize().map_err(|e| i18n::tr_format!("Tresor nicht lesbar: {e}"))?;
        const MAX_VAULT_SIZE: u64 = 256 * 1024 * 1024;
        let file = std::fs::File::open(&path).map_err(|e| i18n::tr_format!("Tresor nicht lesbar: {e}"))?;
        let metadata = file.metadata().map_err(|e| e.to_string())?;
        if !metadata.is_file() || metadata.len() > MAX_VAULT_SIZE { return Err(i18n::tr("Tresordatei ist ungültig oder zu groß").into()); }
        let mut data = Vec::new();
        file.take(MAX_VAULT_SIZE + 1).read_to_end(&mut data).map_err(|e| e.to_string())?;
        if data.len() as u64 > MAX_VAULT_SIZE { return Err(i18n::tr("Tresordatei ist zu groß").into()); }
        if data.len() < 36 || &data[..8] != MAGIC {
            return Err(i18n::tr("Unbekanntes Dateiformat").into());
        }
        let mut salt = [0u8; 16];
        salt.copy_from_slice(&data[8..24]);
        let key = derive(password, &salt)?;
        let cipher = Aes256Gcm::new_from_slice(key.as_ref()).map_err(|e| e.to_string())?;
        let plain = Zeroizing::new(
            cipher
                .decrypt(Nonce::from_slice(&data[24..36]), &data[36..])
                .map_err(|_| i18n::tr("Falsches Passwort").to_string())?,
        );
        let vault: Vault = serde_json::from_slice(&plain).map_err(|e| i18n::tr_format!("Tresor beschädigt: {e}"))?;
        Ok((Self { path, salt, key, session_lock: None }, vault))
    }

    fn encode(&self, vault: &Vault) -> Result<Vec<u8>, String> {
        let plain = Zeroizing::new(serde_json::to_vec(vault).map_err(|e| e.to_string())?);
        let cipher = Aes256Gcm::new_from_slice(self.key.as_ref()).map_err(|e| e.to_string())?;
        let mut nonce = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce);
        let ct = cipher.encrypt(Nonce::from_slice(&nonce), plain.as_ref()).map_err(|e| e.to_string())?;
        let mut out = Vec::with_capacity(36 + ct.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&self.salt);
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ct);
        Ok(out)
    }

    /// Create a complete encrypted vault without ever replacing an existing file.
    pub fn create_new(path: PathBuf, password: &str, vault: &Vault) -> Result<Self, String> {
        let guard = Self::lock_session(&path)?;
        let mut file = Self::create(path, password)?;
        file.session_lock = Some(guard);
        if let Some(dir) = file.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        crate::private_file::write_new(&file.path, &file.encode(vault)?).map_err(|e| e.to_string())?;
        file.path = file.path.canonicalize().map_err(|e| e.to_string())?;
        Ok(file)
    }

    pub fn save(&self, vault: &Vault) -> Result<(), String> {
        let out = self.encode(vault)?;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        // Use an exclusive, private temporary file and flush it before replacing the vault.
        use std::io::Write;
        let tmp = self.path.with_extension(format!("{:016x}.tmp", rand::random::<u64>()));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp).map_err(|e| e.to_string())?;
        let result = (|| {
            file.write_all(&out)?;
            file.sync_all()?;
            drop(file);
            std::fs::rename(&tmp, &self.path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result.map_err(|e| e.to_string())
    }

    /// Changes the vault password and re-encrypts saved PINs with the new key.
    pub fn change_password(&mut self, old: &str, new: &str, vault: &mut Vault) -> Result<(), String> {
        if derive(old, &self.salt)?.as_ref() != self.key.as_ref() {
            return Err(i18n::tr("Aktuelles Passwort ist falsch").into());
        }
        let mut salt = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut salt);
        let candidate = Self { path: self.path.clone(), salt, key: derive(new, &salt)?, session_lock: self.session_lock.clone() };
        let mut updated = vault.clone();
        for access in &mut updated.accesses {
            if let AccessKind::PayPal { credentials, .. } = &mut access.kind {
                let secret = self.open_secret(credentials).ok_or(i18n::tr("PayPal-Zugangsdaten beschädigt; Passwort wurde nicht geändert"))?;
                *credentials = candidate.seal_secret(&secret)?;
            }
            if let Some(sealed) = &access.saved_pin {
                let pin = self.open_secret(sealed).ok_or(i18n::tr("Gespeicherte PIN beschädigt; Passwort wurde nicht geändert"))?;
                access.saved_pin = Some(candidate.seal_secret(&pin)?);
            }
        }
        // Commit in-memory credentials only after the new encrypted vault is safely written.
        candidate.save(&updated)?;
        *self = candidate;
        *vault = updated;
        Ok(())
    }

    /// Separate key for secrets: SHA-256("kontor-secret-v1" || vault key).
    fn secret_key(&self) -> Zeroizing<[u8; 32]> {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(b"kontor-secret-v1");
        h.update(self.key.as_ref());
        let mut k = Zeroizing::new([0u8; 32]);
        k.copy_from_slice(&h.finalize());
        k
    }

    pub fn seal_secret(&self, secret: &str) -> Result<SealedSecret, String> {
        let key = self.secret_key();
        let cipher = Aes256Gcm::new_from_slice(key.as_ref()).map_err(|e| e.to_string())?;
        let mut nonce = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce);
        let data = cipher.encrypt(Nonce::from_slice(&nonce), secret.as_bytes()).map_err(|e| e.to_string())?;
        Ok(SealedSecret { nonce: nonce.to_vec(), data })
    }

    pub fn open_secret(&self, sealed: &SealedSecret) -> Option<Zeroizing<String>> {
        if sealed.nonce.len() != 12 {
            return None;
        }
        let key = self.secret_key();
        let cipher = Aes256Gcm::new_from_slice(key.as_ref()).ok()?;
        let plain = cipher.decrypt(Nonce::from_slice(&sealed.nonce), sealed.data.as_ref()).ok()?;
        String::from_utf8(plain).ok().map(Zeroizing::new)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "subprocess helper, invoked by exclusive_session_survives_password_change"]
    fn session_lock_child() {
        let path=std::env::var_os("BANKINGCRAFT_LOCK_TEST_PATH").expect("test path");
        let expected=std::env::var("BANKINGCRAFT_LOCK_TEST_ALLOWED").unwrap()=="1";
        assert_eq!(VaultFile::lock_session(Path::new(&path)).is_ok(),expected);
    }

    #[test]
    fn exclusive_session_survives_password_change() {
        let dir=std::env::temp_dir().join(format!("bankingcraft-lock-{:016x}",rand::random::<u64>()));
        std::fs::create_dir(&dir).unwrap();
        let path=dir.join("vault.kontor");
        let mut vault=Vault::default();
        let mut file=VaultFile::create_new(path.clone(),"synthetic-old",&vault).unwrap();
        let verify=|allowed:bool| {
            let output=std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact","store::tests::session_lock_child","--ignored"])
                .env("BANKINGCRAFT_LOCK_TEST_PATH",&path)
                .env("BANKINGCRAFT_LOCK_TEST_ALLOWED",if allowed {"1"} else {"0"}).output().unwrap();
            assert!(output.status.success(),"child lock assertion failed");
        };
        verify(false);
        file.change_password("synthetic-old","synthetic-new",&mut vault).unwrap();
        verify(false);
        let (reopened,_)=file.reopen_session("synthetic-new").unwrap();
        drop(file); verify(false); drop(reopened); verify(true);
        #[cfg(unix)] {
            let lock=dir.join("vault.kontor.lock");
            std::fs::remove_file(&lock).unwrap();
            std::os::unix::fs::symlink(&path,&lock).unwrap();
            assert!(VaultFile::lock_session(&path).is_err());
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn native_parser_migration_preserves_duplicate_counts_and_categories() {
        let mut old=crate::business_tests::tx(123,"2026-11-01");
        old.amount="1.2300".parse().unwrap(); old.purpose="Invoice\n  123".into(); old.end_to_end_id="NOTPROVIDED".into(); old.compute_fingerprint();
        let mut new=old.clone();new.amount="1.23".parse().unwrap();new.purpose="Invoice 123".into();new.end_to_end_id.clear();new.compute_fingerprint();
        let mut vault=Vault::default();vault.merge_transactions(1,vec![old.clone(),old]);
        vault.transactions[0].category=Some("Keep".into());
        assert_eq!(vault.merge_aqbanking_transactions(1,vec![new.clone(),new.clone()]),0);
        assert_eq!(vault.transactions[0].category.as_deref(),Some("Keep"));
        assert_eq!(vault.merge_aqbanking_transactions(1,vec![new.clone(),new.clone(),new]),1);
    }

    #[test]
    fn password_change_is_transactional_and_preserves_saved_pin() {
        let dir=std::env::temp_dir().join(format!("bankingcraft-vault-test-{:016x}",rand::random::<u64>()));
        std::fs::create_dir_all(&dir).unwrap();
        let path=dir.join("vault.kontor");
        let mut file=VaultFile::create(path.clone(),"old-password").unwrap();
        let mut vault=Vault::default();
        vault.accesses.push(BankAccess {id:1,name:"Test".into(),kind:AccessKind::PinTan,config:Default::default(),state:Default::default(),saved_pin:Some(file.seal_secret("123456").unwrap())});
        file.save(&vault).unwrap();
        let before=std::fs::read(&path).unwrap();
        assert!(file.change_password("wrong-password","new-password",&mut vault).is_err());
        assert_eq!(before,std::fs::read(&path).unwrap());
        let blocker=dir.join("not-a-directory");
        std::fs::write(&blocker,b"block").unwrap();
        file.path=blocker.join("vault.kontor");
        assert!(file.change_password("old-password","new-password",&mut vault).is_err());
        assert_eq!(file.open_secret(vault.accesses[0].saved_pin.as_ref().unwrap()).unwrap().as_str(),"123456");
        assert_eq!(before,std::fs::read(&path).unwrap());
        file.path=path.clone();
        file.change_password("old-password","new-password",&mut vault).unwrap();
        assert!(VaultFile::open(path.clone(),"old-password").is_err());
        let (reopened,loaded)=VaultFile::open(path.clone(),"new-password").unwrap();
        assert_eq!(reopened.open_secret(loaded.accesses[0].saved_pin.as_ref().unwrap()).unwrap().as_str(),"123456");
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,0o600);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn corrupt_saved_pin_does_not_get_silently_erased_on_password_change() {
        let dir=std::env::temp_dir().join(format!("bankingcraft-corrupt-test-{:016x}",rand::random::<u64>()));
        let path=dir.join("vault.kontor");
        let mut file=VaultFile::create(path.clone(),"old-password").unwrap();
        let mut vault=Vault::default();
        vault.accesses.push(BankAccess {id:1,name:"Test".into(),kind:AccessKind::PinTan,config:Default::default(),state:Default::default(),saved_pin:Some(SealedSecret {nonce:vec![],data:vec![1]})});
        file.save(&vault).unwrap();
        let before=std::fs::read(&path).unwrap();
        assert!(file.change_password("old-password","new-password",&mut vault).is_err());
        assert_eq!(before,std::fs::read(&path).unwrap());
        assert!(vault.accesses[0].saved_pin.is_some());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
