//! Bank parameter data (BPD) and user parameter data (UPD).

use crate::model::{Account, TanMethod};
use crate::response::Response;
use crate::syntax::{latin1_to_string, Field, Segment};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Bpd {
    pub version: u32,
    pub bank_name: String,
    /// Business transaction code (e.g. "HKSAL") -> versions offered by the bank.
    pub versions: HashMap<String, Vec<u32>>,
    /// Business transaction code -> TAN required (from HIPINS).
    pub tan_required: HashMap<String, bool>,
    pub tan_methods: Vec<TanMethod>,
    pub hktan_version: u32,
    pub pain001_formats: Vec<String>,
    pub camt_formats: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Upd {
    pub version: u32,
    pub accounts: Vec<Account>,
}

fn text(f: &Field) -> String {
    f.as_ref().map(|b| latin1_to_string(b)).unwrap_or_default()
}

impl Bpd {
    pub fn update_from(&mut self, resp: &Response) {
        for s in &resp.segments {
            match s.code.as_str() {
                "HIBPA" => {
                    self.version = s.text_or_empty(1, 0).parse().unwrap_or(0);
                    self.bank_name = s.text_or_empty(3, 0);
                }
                "HIPINS" => self.parse_hipins(s),
                "HITANS" => self.parse_hitans(s),
                "HISPAS" => {
                    self.pain001_formats = collect_urns(s, "pain.001");
                    self.add_version(s);
                }
                "HICAZS" => {
                    self.camt_formats = collect_urns(s, "camt.052");
                    self.add_version(s);
                }
                code if code.starts_with("HI") && code.ends_with('S') && code.len() == 6 => self.add_version(s),
                _ => {}
            }
        }
    }

    fn add_version(&mut self, s: &Segment) {
        // HIxxxS -> HKxxx
        let job = format!("HK{}", &s.code[2..5]);
        let v = self.versions.entry(job).or_default();
        if !v.contains(&s.version) {
            v.push(s.version);
            v.sort_unstable();
        }
    }

    fn parse_hipins(&mut self, s: &Segment) {
        let flat = s.flat_from(4);
        let pairs = flat.iter().skip(5).collect::<Vec<_>>();
        for p in pairs.chunks(2) {
            if p.len() == 2 {
                let code = text(p[0]);
                if !code.is_empty() {
                    self.tan_required.insert(code, text(p[1]) == "J");
                }
            }
        }
    }

    fn parse_hitans(&mut self, s: &Segment) {
        let size = match s.version {
            6 => 21,
            7 => 26,
            _ => return, // older versions are not used for PSD2 dialogs
        };
        if s.version < self.hktan_version {
            return;
        }
        if s.version > self.hktan_version {
            self.tan_methods.clear();
            self.hktan_version = s.version;
        }
        let flat = s.flat_from(4);
        for g in flat[3.min(flat.len())..].chunks(size) {
            let t = |k: usize| g.get(k).map(text).unwrap_or_default();
            let sf = t(0);
            if sf.is_empty() || self.tan_methods.iter().any(|m| m.security_function == sf) {
                continue;
            }
            let dk_id = t(3);
            let decoupled = dk_id.to_lowercase().contains("decoupled") || (size == 26 && !t(21).is_empty());
            self.tan_methods.push(TanMethod {
                security_function: sf,
                name: t(5),
                technical_id: t(2),
                dk_id,
                tan_process: t(1),
                decoupled,
                needs_medium: t(18) == "2",
                max_tan_length: t(6).parse().unwrap_or(0),
                hhduc_response_required: t(19) == "J",
                decoupled_poll_wait_first: if size == 26 { t(22).parse().unwrap_or(2) } else { 2 },
                decoupled_poll_wait_next: if size == 26 { t(23).parse().unwrap_or(2) } else { 2 },
            });
        }
    }

    /// Highest version of `job` that both the bank and this client support.
    pub fn version_for(&self, job: &str, supported: &[u32]) -> Option<u32> {
        self.versions
            .get(job)?
            .iter()
            .rev()
            .find(|v| supported.contains(v))
            .copied()
    }

    pub fn supports(&self, job: &str) -> bool {
        self.versions.contains_key(job)
    }

    pub fn needs_tan(&self, job: &str) -> bool {
        self.tan_required.get(job).copied().unwrap_or(false)
    }

    pub fn preferred_pain001(&self) -> String {
        for want in ["pain.001.001.09", "pain.001.001.03", "pain.001.003.03"] {
            if let Some(f) = self.pain001_formats.iter().find(|f| f.contains(want)) {
                return f.clone();
            }
        }
        "urn:iso:std:iso:20022:tech:xsd:pain.001.001.03".into()
    }

    pub fn preferred_camt(&self) -> Option<String> {
        for want in ["camt.052.001.08", "camt.052.001.02"] {
            if let Some(f) = self.camt_formats.iter().find(|f| f.contains(want)) {
                return Some(f.clone());
            }
        }
        self.camt_formats.first().cloned()
    }
}

fn collect_urns(s: &Segment, needle: &str) -> Vec<String> {
    s.flat_from(1)
        .iter()
        .map(text)
        .filter(|t| t.contains(needle))
        .collect()
}

impl Upd {
    pub fn update_from(&mut self, resp: &Response) {
        let mut found = Vec::new();
        for s in &resp.segments {
            match s.code.as_str() {
                "HIUPA" => self.version = s.text_or_empty(3, 0).parse().unwrap_or(0),
                "HIUPD" => {
                    let mut a = Account {
                        account_number: s.text_or_empty(1, 0),
                        sub_account: s.text_or_empty(1, 1),
                        blz: s.text_or_empty(1, 3),
                        ..Default::default()
                    };
                    if s.version >= 6 {
                        a.iban = s.text_or_empty(2, 0);
                        a.kind = s.text_or_empty(4, 0).parse().unwrap_or(0);
                        a.currency = s.text_or_empty(5, 0);
                        a.owner = [s.text_or_empty(6, 0), s.text_or_empty(7, 0)].join(" ").trim().to_string();
                        a.product = s.text_or_empty(8, 0);
                        for i in 10..=s.de_count() {
                            if let Some(c) = s.text(i, 0) {
                                if c.starts_with("HK") {
                                    a.allowed.push(c);
                                }
                            }
                        }
                    } else {
                        a.currency = s.text_or_empty(4, 0);
                        a.owner = s.text_or_empty(5, 0);
                        a.product = s.text_or_empty(7, 0);
                    }
                    if a.currency.is_empty() {
                        a.currency = "EUR".into();
                    }
                    if !a.account_number.is_empty() || !a.iban.is_empty() {
                        found.push(a);
                    }
                }
                "HISPA" => {
                    // J:IBAN:BIC:account:sub:280:blz (repeated)
                    for i in 1..=s.de_count() {
                        let iban = s.text_or_empty(i, 1);
                        let bic = s.text_or_empty(i, 2);
                        let acc = s.text_or_empty(i, 3);
                        for a in self.accounts.iter_mut().chain(found.iter_mut()) {
                            if (!iban.is_empty() && a.iban == iban) || (!acc.is_empty() && a.account_number == acc) {
                                if a.iban.is_empty() {
                                    a.iban = iban.clone();
                                }
                                a.bic = bic.clone();
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        if !found.is_empty() {
            // keep BICs learned earlier
            for f in &mut found {
                if let Some(old) = self.accounts.iter().find(|o| o.account_number == f.account_number && o.iban == f.iban) {
                    if f.bic.is_empty() {
                        f.bic = old.bic.clone();
                    }
                }
            }
            self.accounts = found;
        }
    }
}
