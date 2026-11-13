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
    #[serde(default)]
    pub standing_pain001_formats: Vec<String>,
    pub camt_formats: Vec<String>,
    #[serde(default)]
    pub statement_days: HashMap<String, u32>,
    #[serde(default)]
    pub statement_parameters_known: bool,
    #[serde(default)]
    pub vop_jobs: Vec<String>,
    #[serde(default)]
    pub vop_formats: Vec<String>,
    #[serde(default)]
    pub vop_structured_notice: bool,
    #[serde(default)]
    pub vop_parameters_known: bool,
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
        if resp.first("HIBPA").is_some() {
            // A fresh BPD must also remove capabilities withdrawn by the bank.
            *self = Self { vop_parameters_known: true, statement_parameters_known: true, ..Self::default() };
        }
        for s in &resp.segments {
            match s.code.as_str() {
                "HIBPA" => {
                    self.version = s.text_or_empty(1, 0).parse().unwrap_or(0);
                    self.bank_name = s.text_or_empty(3, 0);
                }
                "HIPINS" => self.parse_hipins(s),
                "HITANS" => self.parse_hitans(s),
                "HIVPPS" if s.version == 1 => {
                    self.vop_parameters_known = true;
                    self.vop_structured_notice = s.text_or_empty(4, 1) == "J";
                    self.vop_formats = s.text_or_empty(4, 5).split_whitespace().map(str::to_owned).collect();
                    self.vop_jobs = s.de(4).iter().skip(6).map(text).filter(|s| !s.is_empty()).collect();
                    self.add_version(s);
                }
                "HISPAS" => {
                    self.pain001_formats = collect_urns(s, "pain.001");
                    self.add_version(s);
                }
                "HICAZS" => {
                    self.camt_formats = collect_urns(s, "camt.052");
                    self.parse_statement_days(s, "HKCAZ");
                    self.add_version(s);
                }
                "HIKAZS" => {
                    self.parse_statement_days(s, "HKKAZ");
                    self.add_version(s);
                }
                "HICDBS" => {
                    let formats = collect_urns(s, "pain.001");
                    if !formats.is_empty() { self.standing_pain001_formats = formats; }
                    self.add_version(s);
                }
                code if (code.starts_with("HI") || code.starts_with("DI")) && code.ends_with('S') && code.len() == 6 => self.add_version(s),
                _ => {}
            }
        }
    }

    fn add_version(&mut self, s: &Segment) {
        // HIxxxS -> HKxxx; proprietary credit-card parameters DIxxxS -> DKxxx.
        let job = format!("{}K{}", &s.code[..1], &s.code[2..5]);
        let v = self.versions.entry(job).or_default();
        if !v.contains(&s.version) {
            v.push(s.version);
            v.sort_unstable();
        }
    }

    fn parse_statement_days(&mut self, s: &Segment, job: &str) {
        // BPDJob header is fields 1–3; StoreDays starts the parameter DEG (4).
        if let Ok(days) = s.text_or_empty(4, 0).parse::<u32>() {
            if days > 0 && days <= 9999 {
                self.statement_days.entry(job.into()).and_modify(|old| *old = (*old).min(days)).or_insert(days);
            }
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
            let plausible = sf.len() == 3 && sf.chars().all(|c| c.is_ascii_digit()) && !t(5).is_empty();
            if !plausible || self.tan_methods.iter().any(|m| m.security_function == sf) {
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
            .copied()
            .filter(|v| supported.contains(v))
            .max()
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
                                if c.len() == 5 && (c.starts_with("HK") || c.starts_with("DK")) && c.bytes().all(|b| b.is_ascii_uppercase()) {
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
        // Process after HIUPD regardless of response ordering. Account numbers
        // alone are not unique across branches/subaccounts.
        for s in resp.find("HISPA") {
            for i in 1..=s.de_count() {
                if s.text_or_empty(i, 0) != "J" { continue; }
                let iban = s.text_or_empty(i, 1);
                let bic = s.text_or_empty(i, 2);
                let number = s.text_or_empty(i, 3);
                let sub = s.text_or_empty(i, 4);
                let blz = s.text_or_empty(i, 6);
                for a in &mut self.accounts {
                    let same = if !iban.is_empty() && !a.iban.is_empty() { a.iban == iban } else {
                        !number.is_empty() && a.account_number == number && a.sub_account == sub
                            && (a.blz.is_empty() || blz.is_empty() || a.blz == blz)
                    };
                    if same {
                        if a.iban.is_empty() { a.iban = iban.clone(); }
                        if a.blz.is_empty() { a.blz = blz.clone(); }
                        if !bic.is_empty() { a.bic = bic.clone(); }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod credit_card_parameters {
    use super::*;
    #[test]
    fn retains_standard_and_proprietary_card_permissions_and_versions() {
        let response = Response::new(crate::syntax::parse_segments(b"HIKKUS:1:1+1+1+1+90:J:J'DIKKUS:2:2+1+1+1+90:J:J'DIKKSS:3:3+1+1+1'HIUPD:4:6+test-card::280:12345678++customer+50+EUR+Owner++Card++DKKKU:1+DKKKS:1+HKKKU:1'").unwrap());
        let mut bpd = Bpd::default(); bpd.update_from(&response);
        assert_eq!(bpd.version_for("HKKKU", &[1]), Some(1));
        assert_eq!(bpd.version_for("DKKKU", &[2]), Some(2));
        assert_eq!(bpd.version_for("DKKKS", &[3]), Some(3));
        assert!(!bpd.supports("HKKSS"));
        let mut upd = Upd::default(); upd.update_from(&response);
        assert_eq!(upd.accounts[0].allowed, vec!["DKKKU", "DKKKS", "HKKKU"]);
    }
}
