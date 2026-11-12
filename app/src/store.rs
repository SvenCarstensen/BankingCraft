//! Encrypted local vault: AES-256-GCM, key derived from the master password with Argon2id.
//! File layout: magic (8) | salt (16) | nonce (12) | ciphertext.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use chrono::NaiveDateTime;
use fints::{Account, Balance, StandingOrder, Transaction};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

const MAGIC: &[u8; 8] = b"KONTOR1\0";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AccessKind {
    PinTan,
    ChipCard { reader: String, record: u8 },
    Demo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BankAccess {
    pub id: u32,
    pub name: String,
    pub kind: AccessKind,
    pub config: fints::Config,
    pub state: fints::State,
    /// PIN saved on request – encrypted separately (see [`VaultFile::seal_secret`]).
    #[serde(default)]
    pub saved_pin: Option<SealedSecret>,
}

/// A secret (PIN) encrypted with AES-256-GCM under a key derived from the vault key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SealedSecret {
    pub nonce: Vec<u8>,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredAccount {
    pub id: u32,
    pub access_id: u32,
    pub account: Account,
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Vault {
    pub next_id: u32,
    pub accesses: Vec<BankAccess>,
    pub accounts: Vec<StoredAccount>,
    pub transactions: Vec<StoredTx>,
    pub standing: Vec<StoredStanding>,
    pub rules: Vec<Rule>,
    pub product_id: String,
    pub product_version: String,
    pub chiptan_reader: String,
}

impl Vault {
    pub fn new_id(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }

    pub fn account(&self, id: u32) -> Option<&StoredAccount> {
        self.accounts.iter().find(|a| a.id == id)
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
    }

    /// Insert or update accounts of an access, returning their ids in the given order.
    pub fn merge_accounts(&mut self, access_id: u32, accounts: Vec<Account>) -> Vec<u32> {
        let mut ids = Vec::new();
        for acc in accounts {
            let existing = self.accounts.iter_mut().find(|s| {
                s.access_id == access_id
                    && ((!acc.iban.is_empty() && s.account.iban == acc.iban)
                        || (!acc.account_number.is_empty() && s.account.account_number == acc.account_number))
            });
            match existing {
                Some(s) => {
                    s.account = acc;
                    ids.push(s.id);
                }
                None => {
                    let id = self.new_id();
                    self.accounts.push(StoredAccount { id, access_id, account: acc, balance: None, last_sync: None });
                    ids.push(id);
                }
            }
        }
        ids
    }

    /// Merge fetched transactions: booked ones are de-duplicated by fingerprint,
    /// pending ones replace the previous pending set of that account.
    pub fn merge_transactions(&mut self, account_id: u32, txs: Vec<Transaction>) -> usize {
        self.transactions.retain(|t| !(t.account_id == account_id && t.tx.pending));
        let mut added = 0;
        let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for t in self.transactions.iter().filter(|t| t.account_id == account_id) {
            *seen.entry(t.tx.fingerprint.clone()).or_default() += 1;
        }
        // identical bookings on the same day are legitimate: compare counts per batch
        let mut batch: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for tx in txs {
            let n = batch.entry(tx.fingerprint.clone()).or_default();
            *n += 1;
            if tx.pending || seen.get(&tx.fingerprint).copied().unwrap_or(0) < *n {
                if !tx.pending {
                    added += 1;
                }
                self.transactions.push(StoredTx { account_id, tx, category: None });
            }
        }
        added
    }

    pub fn last_booking(&self, account_id: u32) -> Option<chrono::NaiveDate> {
        self.transactions
            .iter()
            .filter(|t| t.account_id == account_id && !t.tx.pending)
            .map(|t| t.tx.booking_date)
            .max()
    }
}

pub struct VaultFile {
    path: PathBuf,
    salt: [u8; 16],
    key: Zeroizing<[u8; 32]>,
}

fn derive(password: &str, salt: &[u8; 16]) -> Result<Zeroizing<[u8; 32]>, String> {
    let mut key = Zeroizing::new([0u8; 32]);
    argon2::Argon2::default()
        .hash_password_into(password.as_bytes(), salt, key.as_mut())
        .map_err(|e| e.to_string())?;
    Ok(key)
}

pub fn default_path() -> PathBuf {
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
        Ok(Self { path, salt, key })
    }

    pub fn open(path: PathBuf, password: &str) -> Result<(Self, Vault), String> {
        let data = std::fs::read(&path).map_err(|e| format!("Tresor nicht lesbar: {e}"))?;
        if data.len() < 36 || &data[..8] != MAGIC {
            return Err("Unbekanntes Dateiformat".into());
        }
        let mut salt = [0u8; 16];
        salt.copy_from_slice(&data[8..24]);
        let key = derive(password, &salt)?;
        let cipher = Aes256Gcm::new_from_slice(key.as_ref()).map_err(|e| e.to_string())?;
        let plain = Zeroizing::new(
            cipher
                .decrypt(Nonce::from_slice(&data[24..36]), &data[36..])
                .map_err(|_| "Falsches Passwort".to_string())?,
        );
        let vault: Vault = serde_json::from_slice(&plain).map_err(|e| format!("Tresor beschädigt: {e}"))?;
        Ok((Self { path, salt, key }, vault))
    }

    pub fn save(&self, vault: &Vault) -> Result<(), String> {
        let plain = Zeroizing::new(serde_json::to_vec(vault).map_err(|e| e.to_string())?);
        let cipher = Aes256Gcm::new_from_slice(self.key.as_ref()).map_err(|e| e.to_string())?;
        let mut nonce = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce);
        let ct = cipher.encrypt(Nonce::from_slice(&nonce), plain.as_ref()).map_err(|e| e.to_string())?;
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let mut out = Vec::with_capacity(36 + ct.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&self.salt);
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ct);
        // write atomically
        let tmp = self.path.with_extension("tmp");
        std::fs::write(&tmp, &out).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Changes the vault password and re-encrypts saved PINs with the new key.
    pub fn change_password(&mut self, old: &str, new: &str, vault: &mut Vault) -> Result<(), String> {
        if derive(old, &self.salt)?.as_ref() != self.key.as_ref() {
            return Err("Aktuelles Passwort ist falsch".into());
        }
        let pins: Vec<(u32, Option<Zeroizing<String>>)> = vault
            .accesses
            .iter()
            .map(|a| (a.id, a.saved_pin.as_ref().and_then(|p| self.open_secret(p))))
            .collect();
        let mut salt = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut salt);
        self.key = derive(new, &salt)?;
        self.salt = salt;
        for (id, pin) in pins {
            if let Some(a) = vault.accesses.iter_mut().find(|a| a.id == id) {
                a.saved_pin = match pin {
                    Some(p) => Some(self.seal_secret(&p)?),
                    None => None,
                };
            }
        }
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
